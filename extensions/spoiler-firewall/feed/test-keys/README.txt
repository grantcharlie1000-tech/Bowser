TEST ONLY. NOT A PRODUCTION KEY.

test-feed-key.pem is a throwaway Ed25519 private key committed on purpose so unit tests, the Chrome
end-to-end test and the Rust interop fixtures can sign feeds deterministically. Anyone can sign with it.
It must never be embedded in a shipped build: feed/public-key.txt (the production key) is what the
extension and Bowser trust. The only way the test key is trusted is the unpacked-dev-build override
documented in feed/README.md (a loopback URL plus a storage key that a web page cannot set).
