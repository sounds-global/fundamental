use crate::fns::{length_prefix, length_prefixed, read_labelled_material};
use crate::{
    KeyPurpose, KeyScope, KeyedTag, KeyringError, KeyringKeySettings, KeyringSettings,
    KeyringVersion, OpenRefusal, material_fingerprint,
};
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use fundamental_core::secrets::InMemorySecretString;
use fundamental_secrets::{KvPath, VaultClient};
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use subtle::ConstantTimeEq;
use tracing::field::Empty;
use tracing::{Span, instrument};
use zeroize::Zeroizing;

const FORMAT: u8 = 1;
const HEADER_LEN: usize = 3;
const NONCE_LEN: usize = 24;
const AUTH_TAG_LEN: usize = 16;
const KEY_LEN: usize = 32;

/// What one configured version yields: separate seal and tag subkeys, and a fingerprint.
struct VersionKeys {
    seal: Zeroizing<[u8; KEY_LEN]>,
    tag: Zeroizing<[u8; KEY_LEN]>,
    fingerprint: [u8; KEY_LEN],
}

/// A purpose-typed set of versioned 256-bit keys.
///
/// Tokens are `format(1) ‖ version(2) ‖ nonce(24) ‖ ciphertext ‖ tag(16)` under
/// XChaCha20-Poly1305 with a fresh random nonce. [`open`](Self::open) refuses alike and does the
/// same work for an unheld version, so tokens cannot map which versions exist.
pub struct Keyring<P: KeyPurpose> {
    name: Box<str>,
    current: (KeyringVersion, VersionKeys),
    retained: BTreeMap<KeyringVersion, VersionKeys>,
    /// A key nothing is sealed under, so an unheld version costs the same to refuse.
    unheld: Zeroizing<[u8; KEY_LEN]>,
    purpose: PhantomData<fn() -> P>,
}

impl<P: KeyPurpose> Keyring<P> {
    /// Builds a keyring from configuration.
    ///
    /// Every version's material must read `<purpose>:<version>:<64 hex>` for this purpose and
    /// version.
    ///
    /// # Errors
    /// [`KeyringError::InvalidSettings`] listing **every** problem: no versions; a version
    /// numbered zero or listed twice; material that is unlabelled, labelled for another purpose
    /// or version, not 64 hex characters, or all zero; two versions with identical material; or
    /// a `current` version that is not configured.
    pub fn from_settings(settings: &KeyringSettings) -> Result<Self, KeyringError> {
        Self::named(settings, P::NAME.into())
    }

    /// Builds a keyring kept in OpenBao at `path`: field `current` names the current version and
    /// fields `material_<n>` hold each version up to it.
    ///
    /// # Errors
    /// [`KeyringError::Unreadable`] when the secret cannot be read, its `current` is not a
    /// version, or a version up to it has no material; then as
    /// [`from_settings`](Self::from_settings).
    #[instrument(skip_all, fields(purpose = P::NAME, vault.path = %path, current = Empty))]
    pub async fn from_vault(vault: &VaultClient, path: &KvPath) -> Result<Self, KeyringError> {
        let unreadable = |reason: String| KeyringError::Unreadable {
            purpose: P::NAME,
            path: path.to_string(),
            reason,
        };
        let version = vault
            .read_kv_current_version(path)
            .await
            .map_err(|cause| unreadable(cause.to_string()))?
            .ok_or_else(|| unreadable("OpenBao holds no such secret".to_owned()))?;
        let head = vault
            .read_kv_fields(path, version, &["current"])
            .await
            .map_err(|cause| unreadable(cause.to_string()))?;
        let current = head
            .get("current")
            .and_then(|current| current.parse::<u16>().ok())
            .filter(|current| *current > 0)
            .ok_or_else(|| unreadable("its `current` is not a version".to_owned()))?;
        Span::current().record("current", current);
        let names: Vec<String> = (1..=current).map(|n| format!("material_{n}")).collect();
        let keys: Vec<&str> = names.iter().map(String::as_str).collect();
        let fields = vault
            .read_kv_fields(path, version, &keys)
            .await
            .map_err(|cause| unreadable(cause.to_string()))?;
        let versions = (1..=current)
            .zip(&names)
            .map(|(version, name)| {
                let material = fields
                    .get(name)
                    .ok_or_else(|| unreadable(format!("`{name}` is missing")))?;
                Ok(KeyringKeySettings {
                    version,
                    material: InMemorySecretString::new(material.to_owned()),
                })
            })
            .collect::<Result<Vec<_>, KeyringError>>()?;
        Self::from_settings(&KeyringSettings { current, versions })
    }

    /// Builds a keyring for this purpose in `scope`.
    ///
    /// As [`from_settings`](Self::from_settings), with the purpose
    /// [`scope.qualify(P::NAME)`](KeyScope::qualify) throughout.
    ///
    /// # Errors
    /// As [`from_settings`](Self::from_settings); material labelled for the unscoped purpose or
    /// for another scope is refused like material for another purpose.
    pub fn from_settings_in(
        settings: &KeyringSettings,
        scope: &KeyScope,
    ) -> Result<Self, KeyringError> {
        Self::named(settings, scope.qualify(P::NAME).into_boxed_str())
    }

    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn named(settings: &KeyringSettings, name: Box<str>) -> Result<Self, KeyringError> {
        let mut problems = Vec::new();
        let mut materials: Vec<(KeyringVersion, Zeroizing<[u8; KEY_LEN]>)> =
            Vec::with_capacity(settings.versions.len());
        let mut listed: Vec<KeyringVersion> = Vec::with_capacity(settings.versions.len());

        if settings.versions.is_empty() {
            problems.push("no key versions are configured".to_owned());
        }
        for key in &settings.versions {
            let Some(version) = KeyringVersion::new(key.version) else {
                problems.push("version 0 is reserved; versions start at 1".to_owned());
                continue;
            };
            if listed.contains(&version) {
                problems.push(format!("version {version} is listed more than once"));
                continue;
            }
            listed.push(version);
            let Some(labelled) = key.material.expose() else {
                problems.push(format!("version {version}: the key material was zeroized"));
                continue;
            };
            match read_labelled_material(labelled, &name, &version.to_string()) {
                Ok(material) => materials.push((version, material)),
                Err(problem) => problems.push(format!("version {version}: {problem}")),
            }
        }
        for (index, (first, first_material)) in materials.iter().enumerate() {
            for (second, second_material) in materials.iter().skip(index + 1) {
                if bool::from(first_material.ct_eq(&**second_material)) {
                    problems.push(format!(
                        "versions {first} and {second} carry identical key material"
                    ));
                }
            }
        }
        let current = KeyringVersion::new(settings.current);
        if current.is_none_or(|current| !listed.contains(&current)) {
            problems.push(format!(
                "current version {} is not among the configured versions",
                settings.current
            ));
        }

        if !problems.is_empty() {
            return Err(KeyringError::InvalidSettings {
                purpose: P::NAME,
                problems: format!("as `{name}`: {}", problems.join("; ")),
            });
        }

        let mut retained = BTreeMap::new();
        let mut current_keys = None;
        let mut unheld = None;
        for (version, material) in materials {
            let keys = derive_version_keys(&name, &material);
            if Some(version) == current {
                unheld = Some(Zeroizing::new(blake3::derive_key(
                    &format!("fundamental keyring 2026-09-12 unheld {name}"),
                    material.as_slice(),
                )));
                current_keys = Some((version, keys));
            } else {
                retained.insert(version, keys);
            }
        }
        let (Some(current), Some(unheld)) = (current_keys, unheld) else {
            return Err(KeyringError::InvalidSettings {
                purpose: P::NAME,
                problems: "the current version's material did not parse".to_owned(),
            });
        };
        Ok(Self {
            name,
            current,
            retained,
            unheld,
            purpose: PhantomData,
        })
    }

    /// The purpose as this keyring answers to it, scope included.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The version new tokens and tags are made with.
    #[must_use]
    pub fn current_version(&self) -> KeyringVersion {
        self.current.0
    }

    /// Every version this keyring holds, current first, then retained from newest to oldest.
    pub fn versions(&self) -> impl Iterator<Item = KeyringVersion> + '_ {
        std::iter::once(self.current.0).chain(self.retained.keys().rev().copied())
    }

    /// A non-secret fingerprint of every held version's material, current first.
    ///
    /// Independent of the purpose, so equal material shows an equal fingerprint anywhere.
    pub fn fingerprints(&self) -> impl Iterator<Item = (KeyringVersion, [u8; 32])> + '_ {
        std::iter::once((self.current.0, self.current.1.fingerprint)).chain(
            self.retained
                .iter()
                .rev()
                .map(|(version, keys)| (*version, keys.fingerprint)),
        )
    }

    /// Seals `plaintext` under the current version, authenticating every part of `context`.
    ///
    /// `context` is not stored; [`open`](Self::open) needs the same parts. Pass them separately,
    /// never pre-joined.
    ///
    /// # Errors
    /// [`KeyringError::PlaintextTooLong`] if `plaintext` exceeds the purpose's
    /// `MAX_PLAINTEXT_LEN`, [`KeyringError::Entropy`] if the operating system cannot supply a
    /// nonce, or [`KeyringError::Encryption`] if the cipher refuses.
    #[instrument(
        level = "debug",
        skip_all,
        fields(purpose = %self.name, key_version = self.current.0.get(), plaintext_len = plaintext.len()),
        err(Debug)
    )]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn seal(&self, plaintext: &[u8], context: &[&[u8]]) -> Result<Vec<u8>, KeyringError> {
        if plaintext.len() > P::MAX_PLAINTEXT_LEN {
            return Err(KeyringError::PlaintextTooLong {
                purpose: P::NAME,
                len: plaintext.len(),
                max: P::MAX_PLAINTEXT_LEN,
            });
        }
        let (version, keys) = &self.current;
        let mut nonce = [0_u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| KeyringError::Entropy { purpose: P::NAME })?;
        let header = header(*version);
        let associated = associated_data(&self.name, &header, context);
        let refused = || KeyringError::Encryption { purpose: P::NAME };
        let cipher =
            XChaCha20Poly1305::new_from_slice(keys.seal.as_slice()).map_err(|_| refused())?;
        // Sealed in place in a zeroizing buffer so a refusal leaves no plaintext copy.
        let mut sealed = Zeroizing::new(Vec::with_capacity(plaintext.len() + AUTH_TAG_LEN));
        sealed.extend_from_slice(plaintext);
        cipher
            .encrypt_in_place(&XNonce::from(nonce), &associated, &mut *sealed)
            .map_err(|_| refused())?;

        let mut token = Vec::with_capacity(HEADER_LEN + NONCE_LEN + sealed.len());
        token.extend_from_slice(&header);
        token.extend_from_slice(&nonce);
        token.extend_from_slice(&sealed);
        Ok(token)
    }

    /// Opens a token this keyring sealed, under whichever held version made it.
    ///
    /// A refusal is an expected result, recorded in the span's `outcome`, not an error event.
    ///
    /// # Errors
    /// [`KeyringError::Refused`], whatever the reason.
    #[instrument(
        level = "debug",
        skip_all,
        fields(purpose = %self.name, token_len = token.len(), outcome = Empty)
    )]
    pub fn open(
        &self,
        token: &[u8],
        context: &[&[u8]],
    ) -> Result<Zeroizing<Vec<u8>>, KeyringError> {
        let opened = self.open_or_refuse(token, context);
        Span::current().record(
            "outcome",
            opened
                .as_ref()
                .map_or_else(|reason| reason.as_str(), |_| "opened"),
        );
        opened.map_err(|reason| KeyringError::Refused {
            purpose: P::NAME,
            reason,
        })
    }

    /// The keyed digest of `parts` under the current version.
    #[must_use]
    pub fn tag(&self, parts: &[&[u8]]) -> KeyedTag<P> {
        let (version, keys) = &self.current;
        keyed_tag(*version, keys, parts)
    }

    /// The keyed digest of `parts` under every held version, current first.
    ///
    /// For a lookup that must still find rows tagged before a rotation.
    #[must_use]
    pub fn tags(&self, parts: &[&[u8]]) -> Vec<KeyedTag<P>> {
        std::iter::once(&self.current)
            .map(|(version, keys)| keyed_tag(*version, keys, parts))
            .chain(
                self.retained
                    .iter()
                    .rev()
                    .map(|(version, keys)| keyed_tag(*version, keys, parts)),
            )
            .collect()
    }

    /// The keyed digest of `parts` under a specific held version, or `None` if it is not held.
    #[must_use]
    pub fn tag_with(&self, version: KeyringVersion, parts: &[&[u8]]) -> Option<KeyedTag<P>> {
        self.keys_for(version)
            .map(|keys| keyed_tag(version, keys, parts))
    }

    fn open_or_refuse(
        &self,
        token: &[u8],
        context: &[&[u8]],
    ) -> Result<Zeroizing<Vec<u8>>, OpenRefusal> {
        let shortest = HEADER_LEN + NONCE_LEN + AUTH_TAG_LEN;
        if token.len() < shortest {
            return Err(OpenRefusal::Malformed);
        }
        if token.len() > shortest + P::MAX_PLAINTEXT_LEN {
            return Err(OpenRefusal::TooLong);
        }
        let (header, rest) = token.split_at(HEADER_LEN);
        let [format, high, low] = header else {
            return Err(OpenRefusal::Malformed);
        };
        if *format != FORMAT {
            return Err(OpenRefusal::Malformed);
        }
        let held = KeyringVersion::new(u16::from_be_bytes([*high, *low]))
            .and_then(|version| self.keys_for(version));
        let key = held.map_or(&self.unheld, |keys| &keys.seal);

        let (nonce, sealed) = rest.split_at(NONCE_LEN);
        let nonce = <[u8; NONCE_LEN]>::try_from(nonce).map_err(|_| OpenRefusal::Malformed)?;
        let associated = associated_data(&self.name, header, context);
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice())
            .map_err(|_| OpenRefusal::Unauthenticated)?;
        let mut opened = Zeroizing::new(sealed.to_vec());
        let authenticated = cipher
            .decrypt_in_place(&XNonce::from(nonce), &associated, &mut *opened)
            .is_ok();
        match (held.is_some(), authenticated) {
            (true, true) => Ok(opened),
            (true, false) => Err(OpenRefusal::Unauthenticated),
            (false, _) => Err(OpenRefusal::UnknownVersion),
        }
    }

    fn keys_for(&self, version: KeyringVersion) -> Option<&VersionKeys> {
        if version == self.current.0 {
            Some(&self.current.1)
        } else {
            self.retained.get(&version)
        }
    }
}

impl<P: KeyPurpose> fmt::Debug for Keyring<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keyring")
            .field("purpose", &&*self.name)
            .field("current", &self.current.0)
            .field("retained", &self.retained.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn derive_version_keys(name: &str, material: &[u8; KEY_LEN]) -> VersionKeys {
    VersionKeys {
        seal: Zeroizing::new(blake3::derive_key(
            &format!("fundamental keyring 2026-09-12 seal {name}"),
            material,
        )),
        tag: Zeroizing::new(blake3::derive_key(
            &format!("fundamental keyring 2026-09-12 tag {name}"),
            material,
        )),
        fingerprint: material_fingerprint(material),
    }
}

fn header(version: KeyringVersion) -> [u8; HEADER_LEN] {
    let [high, low] = version.get().to_be_bytes();
    [FORMAT, high, low]
}

/// `count ‖ (len ‖ part)*` over the purpose, the header and each context part.
fn associated_data(name: &str, header: &[u8], context: &[&[u8]]) -> Vec<u8> {
    let parts: Vec<&[u8]> = [name.as_bytes(), header]
        .into_iter()
        .chain(context.iter().copied())
        .collect();
    length_prefixed(parts.into_iter())
}

fn keyed_tag<P: KeyPurpose>(
    version: KeyringVersion,
    keys: &VersionKeys,
    parts: &[&[u8]],
) -> KeyedTag<P> {
    let mut hasher = blake3::Hasher::new_keyed(&keys.tag);
    for part in parts {
        hasher.update(&length_prefix(part.len()));
        hasher.update(part);
    }
    KeyedTag::from_parts(version, *hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {

    /// Tokens relying parties hold were sealed under this framing, so it is pinned byte for byte.
    #[test]
    fn the_associated_data_keeps_the_framing_tokens_were_sealed_under() {
        let header = [1_u8, 0, 7];
        let context: [&[u8]; 2] = [b"zone", b"https://resolver.example"];
        let mut expected = Vec::new();
        expected.extend_from_slice(&4_u64.to_le_bytes());
        for part in [Handles::NAME.as_bytes(), &header, context[0], context[1]] {
            expected.extend_from_slice(&(part.len() as u64).to_le_bytes());
            expected.extend_from_slice(part);
        }
        assert_eq!(
            super::associated_data(Handles::NAME, &header, &context),
            expected
        );
    }
    use super::Keyring;
    use crate::{
        KeyPurpose, KeyScope, KeyringError, KeyringKeySettings, KeyringSettings, KeyringVersion,
        OpenRefusal,
    };
    use fundamental_core::Error;
    use fundamental_core::secrets::InMemorySecretString;

    struct Handles;
    impl KeyPurpose for Handles {
        const NAME: &'static str = "test.handles";
        const MAX_PLAINTEXT_LEN: usize = 64;
    }

    struct Digests;
    impl KeyPurpose for Digests {
        const NAME: &'static str = "test.digests";
        const MAX_PLAINTEXT_LEN: usize = 64;
    }

    const KEY_ONE: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const KEY_TWO: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn labelled<P: KeyPurpose>(version: u16, hex: &str) -> String {
        format!("{}:{version}:{hex}", P::NAME)
    }

    fn settings(current: u16, versions: &[(u16, String)]) -> KeyringSettings {
        KeyringSettings {
            current,
            versions: versions
                .iter()
                .map(|(version, material)| KeyringKeySettings {
                    version: *version,
                    material: InMemorySecretString::new(material.clone()),
                })
                .collect(),
        }
    }

    fn ring<P: KeyPurpose>(current: u16, versions: &[(u16, &str)]) -> Keyring<P> {
        let versions: Vec<(u16, String)> = versions
            .iter()
            .map(|(version, hex)| (*version, labelled::<P>(*version, hex)))
            .collect();
        Keyring::from_settings(&settings(current, &versions)).expect("valid test settings")
    }

    fn refusal(result: Result<impl Sized, KeyringError>) -> OpenRefusal {
        match result {
            Err(KeyringError::Refused { reason, .. }) => reason,
            Err(other) => panic!("expected a refusal, got {other:?}"),
            Ok(_) => panic!("expected a refusal, and the token opened"),
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn scoped(scope: &str, label: &str, hex: &str) -> Result<Keyring<Handles>, KeyringError> {
        Keyring::from_settings_in(
            &settings(1, &[(1, format!("{label}:1:{hex}"))]),
            &KeyScope::new(scope).expect("a well-formed scope"),
        )
    }

    /// What an unscoped keyring made before scopes existed is still what it makes and opens.
    #[test]
    fn an_unscoped_keyring_makes_exactly_what_it_made_before_scopes() {
        let keyring = ring::<Handles>(1, &[(1, KEY_ONE)]);
        assert_eq!(keyring.name(), "test.handles");
        assert_eq!(
            hex(keyring.tag(&[b"a", b"bc"]).as_bytes()),
            "1ec2fba222fd1e1be9bb75ecacdeb9968585ed95af24753d696dbcfc009a0138"
        );
        assert_eq!(
            hex(&keyring
                .fingerprints()
                .next()
                .expect("the current version")
                .1),
            "6776c29e8bffb4a83015e9fa6c84d5541a4b8975991ed7dacd6cfa0f60c60775"
        );
        let sealed_before = "010001ef018c5ae57acf2bf4361e090e7fe9e445a1df79167392bafbfe59d656ba82d396d3ced8eedbc7c9383d2ec85ff74a10e8";
        let token: Vec<u8> = (0..sealed_before.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&sealed_before[at..at + 2], 16).expect("hex"))
            .collect();
        let opened = keyring
            .open(&token, &[b"zone-a"])
            .expect("a pre-scope token opens");
        assert_eq!(opened.as_slice(), b"attribute");
    }

    #[test]
    fn a_scoped_keyring_answers_to_its_qualified_purpose() {
        let keyring = scoped("sandbox", "test.handles@sandbox", KEY_ONE).expect("scoped material");
        assert_eq!(keyring.name(), "test.handles@sandbox");
        let token = keyring.seal(b"attribute", &[b"zone-a"]).expect("seal");
        assert_eq!(
            keyring.open(&token, &[b"zone-a"]).expect("open").as_slice(),
            b"attribute"
        );
    }

    #[test]
    fn material_from_another_scope_refuses_to_boot() {
        for (scope, label) in [
            ("sandbox", "test.handles"),
            ("sandbox", "test.handles@staging"),
            ("staging", "test.handles@sandbox"),
        ] {
            let refused = scoped(scope, label, KEY_ONE).expect_err(label);
            assert_eq!(refused.error_code(), "KEYRING_001");
            assert!(
                refused
                    .to_string()
                    .contains(&format!("labelled for `{label}`")),
                "{refused}"
            );
        }
        let unscoped = Keyring::<Handles>::from_settings(&settings(
            1,
            &[(1, format!("test.handles@sandbox:1:{KEY_ONE}"))],
        ))
        .expect_err("scoped material unscoped");
        assert!(
            unscoped
                .to_string()
                .contains("labelled for `test.handles@sandbox`")
        );
    }

    /// Relabelled bytes share nothing across deployments except the fingerprint.
    #[test]
    fn the_same_material_in_two_deployments_shares_no_tag_and_no_token() {
        let production = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let sandbox = scoped("sandbox", "test.handles@sandbox", KEY_ONE).expect("scoped");
        let staging = scoped("staging", "test.handles@staging", KEY_ONE).expect("scoped");

        let parts: [&[u8]; 2] = [b"did:example:person", b"https://verifier.example"];
        assert_ne!(production.tag(&parts), sandbox.tag(&parts));
        assert_ne!(sandbox.tag(&parts), staging.tag(&parts));

        for (sealer, opener) in [
            (&production, &sandbox),
            (&sandbox, &production),
            (&sandbox, &staging),
        ] {
            let token = sealer.seal(b"attribute", &[b"zone-a"]).expect("seal");
            assert_eq!(
                refusal(opener.open(&token, &[b"zone-a"])),
                OpenRefusal::Unauthenticated,
                "a {} token opened as {}",
                sealer.name(),
                opener.name()
            );
        }
        assert_eq!(
            production.fingerprints().next().map(|(_, print)| print),
            sandbox.fingerprints().next().map(|(_, print)| print)
        );
    }

    #[test]
    fn a_sealed_token_opens_to_its_plaintext() {
        let keyring = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let token = keyring.seal(b"attribute", &[b"zone-a"]).expect("seal");
        let opened = keyring.open(&token, &[b"zone-a"]).expect("open");
        assert_eq!(opened.as_slice(), b"attribute");
    }

    #[test]
    fn two_seals_of_one_plaintext_share_nothing() {
        let keyring = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let first = keyring.seal(b"attribute", &[]).expect("seal");
        let second = keyring.seal(b"attribute", &[]).expect("seal");
        assert_eq!(first.len(), second.len());
        assert_ne!(first[3..], second[3..], "only the header may repeat");
    }

    #[test]
    fn an_altered_token_is_refused_as_unauthenticated() {
        let keyring = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let mut token = keyring.seal(b"attribute", &[]).expect("seal");
        if let Some(last) = token.last_mut() {
            *last ^= 1;
        }
        let refused = keyring.open(&token, &[]).expect_err("an altered token");
        assert_eq!(refused.error_code(), "KEYRING_002");
        assert_eq!(refusal(Err::<(), _>(refused)), OpenRefusal::Unauthenticated);
    }

    #[test]
    fn a_token_is_valid_only_for_the_context_it_was_sealed_for() {
        let keyring = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let token = keyring.seal(b"attribute", &[b"zone-a"]).expect("seal");
        assert_eq!(
            refusal(keyring.open(&token, &[b"zone-b"])),
            OpenRefusal::Unauthenticated
        );
    }

    #[test]
    fn context_part_boundaries_are_authenticated() {
        let keyring = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let token = keyring.seal(b"attribute", &[b"ab", b"c"]).expect("seal");
        assert_eq!(
            refusal(keyring.open(&token, &[b"a", b"bc"])),
            OpenRefusal::Unauthenticated
        );
        assert_eq!(
            refusal(keyring.open(&token, &[b"abc"])),
            OpenRefusal::Unauthenticated
        );
    }

    #[test]
    fn one_material_under_two_purposes_yields_unrelated_keys() {
        let handles = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let digests = ring::<Digests>(1, &[(1, KEY_ONE)]);
        let token = handles.seal(b"attribute", &[]).expect("seal");
        let reopened = Keyring::<Handles>::from_settings(&settings(
            1,
            &[(1, labelled::<Handles>(1, KEY_TWO))],
        ))
        .expect("valid");
        assert!(reopened.open(&token, &[]).is_err());
        assert_ne!(
            handles.tag(&[b"x"]).as_bytes(),
            digests.tag(&[b"x"]).as_bytes()
        );
    }

    #[test]
    fn material_labelled_for_another_purpose_refuses_to_boot() {
        let refused = Keyring::<Digests>::from_settings(&settings(
            1,
            &[(1, labelled::<Handles>(1, KEY_ONE))],
        ))
        .expect_err("a handle key in a digest keyring");
        assert_eq!(refused.error_code(), "KEYRING_001");
        assert!(
            refused.to_string().contains("labelled for `test.handles`"),
            "{refused}"
        );
    }

    #[test]
    fn material_labelled_as_another_version_refuses_to_boot() {
        let refused = Keyring::<Handles>::from_settings(&settings(
            2,
            &[(2, labelled::<Handles>(1, KEY_ONE))],
        ))
        .expect_err("version 1's material in version 2's slot");
        assert!(
            refused.to_string().contains("labelled as version `1`"),
            "{refused}"
        );
    }

    #[test]
    fn unlabelled_material_refuses_to_boot() {
        let refused = Keyring::<Handles>::from_settings(&settings(1, &[(1, KEY_ONE.to_owned())]))
            .expect_err("bare hex");
        assert!(
            refused
                .to_string()
                .contains("expected `<purpose>:<version>:<64 hex>`")
        );
        assert!(
            !refused.to_string().contains(KEY_ONE),
            "material leaked: {refused}"
        );
    }

    #[test]
    fn a_retained_version_still_opens_after_rotation() {
        let before = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let token = before.seal(b"attribute", &[]).expect("seal");
        let after = ring::<Handles>(2, &[(1, KEY_ONE), (2, KEY_TWO)]);
        assert_eq!(
            after.open(&token, &[]).expect("open").as_slice(),
            b"attribute"
        );
        let resealed = after.seal(b"attribute", &[]).expect("seal");
        assert_eq!(resealed[1..3], 2_u16.to_be_bytes());
    }

    #[test]
    fn an_unheld_version_is_refused_with_the_same_code() {
        let before = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let token = before.seal(b"attribute", &[]).expect("seal");
        let refused = ring::<Handles>(2, &[(2, KEY_TWO)])
            .open(&token, &[])
            .expect_err("version 1 is not held");
        assert_eq!(refused.error_code(), "KEYRING_002");
        assert_eq!(refused.to_string(), "The test.handles token was refused");
        assert_eq!(refusal(Err::<(), _>(refused)), OpenRefusal::UnknownVersion);
    }

    #[test]
    fn the_reason_never_reaches_the_message() {
        let keyring = ring::<Handles>(1, &[(1, KEY_ONE)]);
        for token in [vec![0_u8; 10], vec![0_u8; 200], {
            let mut token = keyring.seal(b"attribute", &[]).expect("seal");
            token[2] = 9;
            token
        }] {
            let message = keyring.open(&token, &[]).expect_err("refused").to_string();
            assert_eq!(message, "The test.handles token was refused");
        }
    }

    #[test]
    fn a_truncated_or_foreign_token_is_malformed() {
        let keyring = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let token = keyring.seal(b"attribute", &[]).expect("seal");
        assert_eq!(
            refusal(keyring.open(&token[..20], &[])),
            OpenRefusal::Malformed
        );
        let mut foreign = token;
        foreign[0] = 9;
        assert_eq!(refusal(keyring.open(&foreign, &[])), OpenRefusal::Malformed);
    }

    #[test]
    fn a_token_longer_than_the_purpose_allows_is_refused_before_any_work() {
        let keyring = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let oversized = vec![1_u8; 3 + 24 + 16 + Handles::MAX_PLAINTEXT_LEN + 1];
        assert_eq!(refusal(keyring.open(&oversized, &[])), OpenRefusal::TooLong);
    }

    #[test]
    fn a_plaintext_longer_than_the_purpose_allows_is_not_sealed() {
        let keyring = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let refused = keyring
            .seal(&[0_u8; Handles::MAX_PLAINTEXT_LEN + 1], &[])
            .expect_err("too long");
        assert_eq!(refused.error_code(), "KEYRING_003");
    }

    #[test]
    fn every_settings_problem_is_reported_at_once() {
        let refused = Keyring::<Handles>::from_settings(&settings(
            5,
            &[
                (0, labelled::<Handles>(0, KEY_ONE)),
                (1, "not labelled".to_owned()),
                (2, labelled::<Handles>(2, &"0".repeat(64))),
                (3, labelled::<Handles>(3, KEY_TWO)),
                (4, labelled::<Handles>(4, KEY_TWO)),
                (4, labelled::<Handles>(4, KEY_ONE)),
                (6, labelled::<Digests>(6, KEY_ONE)),
                (7, labelled::<Handles>(8, KEY_ONE)),
            ],
        ))
        .expect_err("eight problems");
        assert_eq!(refused.error_code(), "KEYRING_001");
        let KeyringError::InvalidSettings { problems, .. } = refused else {
            panic!("expected InvalidSettings, got {refused:?}");
        };
        for expected in [
            "version 0 is reserved",
            "version 1: expected `<purpose>:<version>:<64 hex>`",
            "version 2: an all-zero key is a placeholder",
            "version 4 is listed more than once",
            "versions 3 and 4 carry identical key material",
            "version 6: the material is labelled for `test.digests`",
            "version 7: the material is labelled as version `8`",
            "current version 5 is not among",
        ] {
            assert!(
                problems.contains(expected),
                "missing {expected:?} in {problems}"
            );
        }
    }

    #[test]
    fn an_empty_keyring_is_refused() {
        let refused = Keyring::<Handles>::from_settings(&settings(1, &[]));
        assert!(matches!(refused, Err(KeyringError::InvalidSettings { .. })));
    }

    #[test]
    fn a_tag_is_deterministic_and_carries_its_version() {
        let keyring = ring::<Digests>(1, &[(1, KEY_ONE)]);
        let tag = keyring.tag(&[b"zone", b"grant", b"attribute"]);
        assert_eq!(tag, keyring.tag(&[b"zone", b"grant", b"attribute"]));
        assert_eq!(tag.version().get(), 1);
    }

    #[test]
    fn part_boundaries_are_part_of_the_tag() {
        let keyring = ring::<Digests>(1, &[(1, KEY_ONE)]);
        assert_ne!(keyring.tag(&[b"ab", b"c"]), keyring.tag(&[b"a", b"bc"]));
    }

    #[test]
    fn tags_under_every_version_list_the_current_first() {
        let keyring = ring::<Digests>(2, &[(1, KEY_ONE), (2, KEY_TWO)]);
        let tags = keyring.tags(&[b"attribute"]);
        let versions: Vec<u16> = tags.iter().map(|tag| tag.version().get()).collect();
        assert_eq!(versions, [2, 1]);
        let before = ring::<Digests>(1, &[(1, KEY_ONE)]).tag(&[b"attribute"]);
        assert_eq!(
            tags[1], before,
            "a row written before rotation is still found"
        );
        let version = KeyringVersion::new(1).expect("non-zero");
        assert_eq!(keyring.tag_with(version, &[b"attribute"]), Some(before));
    }

    #[test]
    fn fingerprints_match_across_purposes_and_reveal_no_material() {
        let handles = ring::<Handles>(1, &[(1, KEY_ONE)]);
        let digests = ring::<Digests>(1, &[(1, KEY_ONE)]);
        let other = ring::<Digests>(1, &[(1, KEY_TWO)]);
        let fingerprint = |keyring: &dyn Fn() -> Vec<[u8; 32]>| keyring();
        let of_handles = fingerprint(&|| handles.fingerprints().map(|(_, f)| f).collect());
        let of_digests = fingerprint(&|| digests.fingerprints().map(|(_, f)| f).collect());
        let of_other = fingerprint(&|| other.fingerprints().map(|(_, f)| f).collect());
        assert_eq!(
            of_handles, of_digests,
            "one material, two purposes, one fingerprint"
        );
        assert_ne!(of_handles, of_other);
        assert_ne!(
            of_handles[0], [0x11; 32],
            "the fingerprint is not the material"
        );
    }

    #[test]
    fn debug_names_versions_and_never_material() {
        let rendered = format!("{:?}", ring::<Handles>(2, &[(1, KEY_ONE), (2, KEY_TWO)]));
        assert!(rendered.contains("test.handles"), "{rendered}");
        assert!(!rendered.contains("1111"), "{rendered}");
    }
}
