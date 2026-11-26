use x509_parser::oid_registry::{
    OID_EC_P256, OID_KEY_TYPE_EC_PUBLIC_KEY, OID_SIG_ECDSA_WITH_SHA256,
    OID_X509_EXT_AUTHORITY_KEY_IDENTIFIER, OID_X509_EXT_BASIC_CONSTRAINTS,
    OID_X509_EXT_EXTENDED_KEY_USAGE, OID_X509_EXT_KEY_USAGE, OID_X509_EXT_NAME_CONSTRAINTS,
    OID_X509_EXT_SUBJECT_KEY_IDENTIFIER,
};
use x509_parser::prelude::{FromDer, GeneralName, X509Certificate};

use crate::LocalError;

/// What a development root says about itself, read from its bytes: the `.test` apexes its name
/// constraint permits, and whether it is exactly the profile `fun local` mints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootProfile {
    permitted: Vec<String>,
}

/// keyCertSign (bit 5) and cRLSign (bit 6), as x509-parser numbers the flags.
const CA_KEY_USAGE: u16 = (1 << 5) | (1 << 6);

impl RootProfile {
    /// Reads `der` and refuses anything but exactly the minted profile.
    ///
    /// # Errors
    ///
    /// [`LocalError::LeafCoversTooMuch`] (`LOCAL_006`, probe `root-profile`) naming each difference.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub fn exact(der: &[u8]) -> Result<Self, LocalError> {
        let refuse = |detail: String| LocalError::LeafCoversTooMuch {
            probe: "root-profile".to_owned(),
            detail,
        };
        let (rest, certificate) =
            X509Certificate::from_der(der).map_err(|error| refuse(error.to_string()))?;
        if !rest.is_empty() {
            return Err(refuse("trailing bytes after the certificate".to_owned()));
        }
        let mut problems = Vec::new();
        if certificate.version().0 != 2 {
            problems.push("not X.509 v3");
        }
        if certificate.subject() != certificate.issuer() {
            problems.push("not self-issued");
        }
        if certificate.signature_algorithm.algorithm != OID_SIG_ECDSA_WITH_SHA256 {
            problems.push("not signed with ecdsa-with-SHA256");
        }
        let key = certificate.public_key();
        let curve = key
            .algorithm
            .parameters
            .as_ref()
            .and_then(|parameters| parameters.as_oid().ok());
        if key.algorithm.algorithm != OID_KEY_TYPE_EC_PUBLIC_KEY || curve != Some(OID_EC_P256) {
            problems.push("not an ECDSA P-256 key");
        }
        let allowed = [
            OID_X509_EXT_BASIC_CONSTRAINTS,
            OID_X509_EXT_KEY_USAGE,
            OID_X509_EXT_EXTENDED_KEY_USAGE,
            OID_X509_EXT_NAME_CONSTRAINTS,
            OID_X509_EXT_SUBJECT_KEY_IDENTIFIER,
            OID_X509_EXT_AUTHORITY_KEY_IDENTIFIER,
        ];
        if certificate
            .extensions()
            .iter()
            .any(|extension| !allowed.contains(&extension.oid))
        {
            problems.push("carries an extension the profile does not");
        }
        match certificate.basic_constraints() {
            Ok(Some(basic))
                if basic.critical
                    && basic.value.ca
                    && basic.value.path_len_constraint == Some(0) => {}
            _ => problems.push("basic constraints are not critical CA:true, pathlen:0"),
        }
        match certificate.key_usage() {
            Ok(Some(usage)) if usage.critical && usage.value.flags == CA_KEY_USAGE => {}
            _ => problems.push("key usage is not critical keyCertSign, cRLSign"),
        }
        match certificate.extended_key_usage() {
            Ok(Some(usage))
                if usage.value.server_auth
                    && !usage.value.any
                    && !usage.value.client_auth
                    && !usage.value.code_signing
                    && !usage.value.email_protection
                    && !usage.value.time_stamping
                    && !usage.value.ocsp_signing
                    && usage.value.other.is_empty() => {}
            _ => problems.push("extended key usage is not exactly serverAuth"),
        }
        let mut permitted = Vec::new();
        match certificate.name_constraints() {
            Ok(Some(constraints)) if constraints.critical => {
                for subtree in constraints.value.permitted_subtrees.iter().flatten() {
                    match subtree.base {
                        GeneralName::DNSName(name) => {
                            permitted.push(name.to_owned());
                        }
                        _ => problems.push("permits something other than a DNS name"),
                    }
                }
                let mut excluded: Vec<&[u8]> = constraints
                    .value
                    .excluded_subtrees
                    .iter()
                    .flatten()
                    .map(|subtree| match subtree.base {
                        GeneralName::IPAddress(bytes) => bytes,
                        _ => &[],
                    })
                    .collect();
                excluded.sort();
                if excluded != [&[0_u8; 8][..], &[0_u8; 32][..]] {
                    problems.push("does not exclude exactly every IPv4 and IPv6 address");
                }
            }
            _ => problems.push("has no critical name constraint"),
        }
        if permitted.is_empty() {
            problems.push("permits no name");
        }
        if problems.is_empty() {
            permitted.sort();
            Ok(Self { permitted })
        } else {
            Err(refuse(problems.join("; ")))
        }
    }

    /// The names the root's constraint permits, sorted.
    #[must_use]
    pub fn permitted(&self) -> &[String] {
        &self.permitted
    }
}
