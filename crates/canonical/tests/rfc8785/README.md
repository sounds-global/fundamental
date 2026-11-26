# RFC 8785 test data

Copied byte for byte from the RFC author's reference repository, which is Apache-2.0
(`LICENSE`, copyright Anders Rundgren):

- source: https://github.com/cyberphone/json-canonicalization, `testdata/input` and `testdata/output`
- commit: `19d51d7fe467d4706a3ff08adf8a748f29fc21e0` (2024-12-13)
- `SHA256SUMS` pins every file; `tests/rfc8785_vectors.rs` refuses to run on a file that differs

Every `output/` file is canonical by definition. Five are inside the I-JSON integer profile
`fundamental_canonical` reads; `values.json` carries floats and is a refusal vector.
`unicode.json` holds an unnormalised `Å`: canonical, accepted by the verifier profile, refused
by the registry profile.
