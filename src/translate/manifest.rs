//! The pinned language files: what Bowser may download for translation, from where, and the
//! SHA-256 each file must have. Nothing else is ever fetched, and nothing is fetched without the user
//! asking to translate. The table was generated from the sources below and is checked by tests; never
//! hand-edit a hash.
//!
//! ## Sources
//!
//! * Engine: the BrowserMT `bergamot-translator` v0.4.5 WebAssembly build
//!   (`bergamot-translator-worker.js` and `.wasm`) from its GitHub release
//!   <https://github.com/browsermt/bergamot-translator/releases/tag/v0.4.5>. The release publishes no
//!   checksums; the hashes below are the SHA-256 of the files as downloaded on 2026-10-15, which were
//!   then run headless against every pinned pair (all 17 translated a sentence, and a pivot through
//!   English worked).
//! * Models: Mozilla's Firefox Translations models, release `1.0` of the Remote Settings collection
//!   `translations-models`
//!   (<https://firefox.settings.services.mozilla.com/v1/buckets/main/collections/translations-models/records>),
//!   each file at `https://firefox-settings-attachments.cdn.mozilla.net/<attachment.location>`. The
//!   hash is the record's `attachment.hash`, and every file was downloaded again and hashed
//!   independently before being pinned. A pair is the quantised `model.*.intgemm.alphas.bin`, its
//!   `lex.50.50.*.s2t.bin` shortlist and the shared `vocab.*.spm`; on disk the names are normalised to
//!   `model.bin`, `lex.bin` and `vocab.spm` under `models/<src>-<dst>/`.
//!
//! ## Licence
//!
//! Both are MPL-2.0. Engine: the `bergamot-translator` repository's licence is MPL-2.0 (GitHub's
//! licence API reports `mpl-2.0`), and Mozilla's Remote Settings record for the same release says
//! `"license": "MPL-2.0"`. Models: the `mozilla/translations` README says "The model files are
//! distributed under the MPL 2.0 license", and `mozilla/firefox-translations-models` carries the MPL-2.0
//! `LICENSE` text. MPL-2.0 permits fetching and running these files unchanged, which is all Bowser
//! does: it ships none of them and modifies none, and `docs/translate.md` keeps the notice. So every
//! pinned pair is allowed; none was left out for its licence.
//!
//! ## Which pairs
//!
//! Release 1.0 has `<language>-en` and `en-<language>` for most languages. Nine are pinned: German,
//! Spanish, French, Italian, Dutch, Polish, Portuguese and Turkish both ways, and Russian into English
//! only (1.0 has no English-to-Russian model). That is 17 pairs of about 26 to 33 MB each, fetched only
//! when a route needs them. Any other pair goes through English when both halves exist (see
//! `pair_route`). Release `1.0` is used rather than the newer `2.x` ones because it is what Firefox ran
//! with this engine version, and it is the smaller model.
//!
//! The host serves the files, not Bowser: if one is ever removed, a copy placed by hand in the cache
//! folder is accepted after the same size and hash check (see `store`).

/// One file to fetch: its place under the cache folder, where from, and what it must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File {
    /// Relative to the cache folder, with `/` separators.
    pub path: &'static str,
    pub url: &'static str,
    /// Exact size in bytes. A download larger than this is refused, and the result is checked.
    pub size: u64,
    /// Lower-case hex SHA-256.
    pub sha256: &'static str,
}

/// A translation model for one direction. `files` are the model, the lexical shortlist and the
/// vocabulary, in that order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    /// `<src>-<dst>`, also its folder name under `models/`.
    pub id: &'static str,
    pub src: &'static str,
    pub dst: &'static str,
    pub files: [File; 3],
}

/// Everything that may be downloaded.
#[derive(Debug)]
pub struct Manifest {
    pub engine: &'static [File],
    pub pairs: &'static [Pair],
}

impl Manifest {
    pub fn pair(&self, src: &str, dst: &str) -> Option<&'static Pair> {
        self.pairs.iter().find(|pair| pair.src == src && pair.dst == dst)
    }

    /// Languages with at least one pinned pair, `en` first.
    pub fn languages(&self) -> Vec<&'static str> {
        let mut languages = vec!["en"];
        for pair in self.pairs {
            for code in [pair.src, pair.dst] {
                if !languages.contains(&code) {
                    languages.push(code);
                }
            }
        }
        languages
    }
}

pub static MANIFEST: Manifest = Manifest {
    engine: &[
        File {
            path: "engine/bergamot-translator-worker.js",
            url: "https://github.com/browsermt/bergamot-translator/releases/download/v0.4.5/bergamot-translator-worker.js",
            size: 82410,
            sha256: "c119d3af0995aae1747c904f8db8e49447144330e4c7bf0ace410a6a0e22edf0",
        },
        File {
            path: "engine/bergamot-translator-worker.wasm",
            url: "https://github.com/browsermt/bergamot-translator/releases/download/v0.4.5/bergamot-translator-worker.wasm",
            size: 5175782,
            sha256: "492c78592c29362ba27c22e5bf1a6a1e70af46341af4089dcffdf99cee1527dc",
        },
    ],
    pairs: &[
        Pair {
            id: "de-en",
            src: "de",
            dst: "en",
            files: [
                File {
                    path: "models/de-en/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/8e3df11a-c2fc-422a-bc2d-ae234dab44c9.bin",
                    size: 17140837,
                    sha256: "1980225d00dc5645491777accff5b3c9d20b92eff67a25135f1cf8fe2ed8fb8f",
                },
                File {
                    path: "models/de-en/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/e2f7beb4-b45d-49da-b83e-ff421ad1268c.bin",
                    size: 5047568,
                    sha256: "2f7c0f7bbce97ae5b52454074a892ba7b7610fb98e3c5d341e4ca79f0850c4de",
                },
                File {
                    path: "models/de-en/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/9fb51529-8473-4138-a9e0-0a588ab574f0.spm",
                    size: 784269,
                    sha256: "417668f2ed297970febafb5b079a9d5ebc4ed0b3550ac8386d67a90473a09bd7",
                },
            ],
        },
        Pair {
            id: "en-de",
            src: "en",
            dst: "de",
            files: [
                File {
                    path: "models/en-de/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/8a830d1c-e167-4238-8473-4065c65c02d2.bin",
                    size: 17140835,
                    sha256: "b3e980d6602ab0bdfe8d9315cb5fc282a16bb1c8dccf38e70945c584551c4318",
                },
                File {
                    path: "models/en-de/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/db204e54-8565-4bba-aa52-1e857bcc68a5.bin",
                    size: 3943644,
                    sha256: "f03eb8245042feb7a5800815b8d0dc215d7a60691632405b65c461d250cedbe6",
                },
                File {
                    path: "models/en-de/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/a5117ec6-0b87-4ae1-9c2e-5151de59af1d.spm",
                    size: 784269,
                    sha256: "417668f2ed297970febafb5b079a9d5ebc4ed0b3550ac8386d67a90473a09bd7",
                },
            ],
        },
        Pair {
            id: "en-es",
            src: "en",
            dst: "es",
            files: [
                File {
                    path: "models/en-es/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/50fbae09-5219-4393-ad75-28b23f44a17d.bin",
                    size: 17140755,
                    sha256: "fa7460037a3163e03fe1d23602f964bff2331da6ee813637e092ddf37156ef53",
                },
                File {
                    path: "models/en-es/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/d2d8de5b-b41e-4eb0-b978-dd48725e3e77.bin",
                    size: 3347104,
                    sha256: "3a113d713dec3cf1d12bba5b138ae616e28bba4bbc7fe7fd39ba145e26b86d7f",
                },
                File {
                    path: "models/en-es/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/e9c774ba-69ff-4951-8783-895aeeb05439.spm",
                    size: 825463,
                    sha256: "909b1eea1face0d7f90a474fe29a8c0fef8d104b6e41e65616f864c964ba8845",
                },
            ],
        },
        Pair {
            id: "en-fr",
            src: "en",
            dst: "fr",
            files: [
                File {
                    path: "models/en-fr/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/762a23ff-317e-4f0d-95ff-e9d95674a521.bin",
                    size: 17140961,
                    sha256: "0678019c4d74c8c81d2de17e3e58d3aba5f5eb48f5595d9240c17f69d30461de",
                },
                File {
                    path: "models/en-fr/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/7178ceaa-3ae6-49cb-a7f0-a72bdded2d9b.bin",
                    size: 7886500,
                    sha256: "38fb44bad1fd5f1e6bfdcf15cc8baa09d61aad2a4f9c587914e24e7b5c25c32c",
                },
                File {
                    path: "models/en-fr/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/808114a9-eff2-47c3-a981-b7119105c40c.spm",
                    size: 831382,
                    sha256: "4c84b95b62c930f0791466d73eb996841eef474c96d0c2f0e6c6d80640f2005a",
                },
            ],
        },
        Pair {
            id: "en-it",
            src: "en",
            dst: "it",
            files: [
                File {
                    path: "models/en-it/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/efb14526-aff4-4826-8cf6-12e2bc44ac7d.bin",
                    size: 17140899,
                    sha256: "3d7bbc4d7977e10b35f53faa79f5d5de8211f4f04baed9e7cd9dee1dcceda917",
                },
                File {
                    path: "models/en-it/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/d683f5a2-7a75-444c-a581-22b3c3cfaecb.bin",
                    size: 4495004,
                    sha256: "351ea80fb9f366f07533c7c4836248e72d9d4aa4eb7a05b5d74891a7abb4208c",
                },
                File {
                    path: "models/en-it/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/8d28b170-7607-4834-a548-2a1244f35576.spm",
                    size: 812781,
                    sha256: "603f3349657c3deb9736a0c567452d102a5a03c377dfdf1d32c428608f2cff1b",
                },
            ],
        },
        Pair {
            id: "en-nl",
            src: "en",
            dst: "nl",
            files: [
                File {
                    path: "models/en-nl/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/668ec699-5377-452e-89b9-7195d58f0047.bin",
                    size: 17140899,
                    sha256: "906690a58a0d72aff28bd4b941cbd0984d1e0a62958c0b21aebae378a656d822",
                },
                File {
                    path: "models/en-nl/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/3055493f-e59d-41f8-a681-823e3ab5ef8d.bin",
                    size: 4494892,
                    sha256: "f780a6d74af4b141f551dcc0da56bab44a05a90ef53d63381269710f35eaa41b",
                },
                File {
                    path: "models/en-nl/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/18a0a000-37d5-45df-8952-6c19e0e65458.spm",
                    size: 807541,
                    sha256: "43ba3922c3bba2b76ca2e2124837c96518b0e31300b7d6d5ccce55ee10d86393",
                },
            ],
        },
        Pair {
            id: "en-pl",
            src: "en",
            dst: "pl",
            files: [
                File {
                    path: "models/en-pl/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/62448fcb-464a-474b-b56a-37686dfe80bf.bin",
                    size: 17140899,
                    sha256: "60d45f43a5ac869a80f899751d2d1f0f456da9815d26db70e4d2e0fd18ed4a8f",
                },
                File {
                    path: "models/en-pl/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/6b14f949-6e9e-44ff-80e5-815a07615a37.bin",
                    size: 3642112,
                    sha256: "409fbf5856cec372dffe0a3aa3c89462e2efbd783557272af84800a67195c38c",
                },
                File {
                    path: "models/en-pl/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/e1fa6366-7675-4d1f-992b-5b1d88b9cb4d.spm",
                    size: 822587,
                    sha256: "a1d27e6f5c0d29f406364ebf0382949d1c0affc750cec4380f3173150552f43e",
                },
            ],
        },
        Pair {
            id: "en-pt",
            src: "en",
            dst: "pt",
            files: [
                File {
                    path: "models/en-pt/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/b268bf87-94b6-4893-9da1-c4e75284ace7.bin",
                    size: 17140899,
                    sha256: "8fb05a27509bea3f67d2f59506485584d5cdbdcafa82b251576c27e91bd7011e",
                },
                File {
                    path: "models/en-pt/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/be3a2e24-b12e-4c0e-b07a-c7b0ba6ab421.bin",
                    size: 4345620,
                    sha256: "dd0e64865277f0a1cf15a26a5b50e9e0570235afd8dab7c5e0249ad7057531d6",
                },
                File {
                    path: "models/en-pt/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/745bff57-f929-41d9-8f0f-913513cfd334.spm",
                    size: 817234,
                    sha256: "2326a27577b5ffa0b822c55fe850986961728097521de35fcc186f7d6dce72d4",
                },
            ],
        },
        Pair {
            id: "en-tr",
            src: "en",
            dst: "tr",
            files: [
                File {
                    path: "models/en-tr/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/f48948a5-06b1-465b-a0dc-ea1a1a52323d.bin",
                    size: 17141051,
                    sha256: "ea3cd4ee09de190df265c78d84260e277a9a8a3ddff05eb55d72afe7fc0fdbf4",
                },
                File {
                    path: "models/en-tr/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/ed96fb89-d560-415b-9391-e5dc8edd13fa.bin",
                    size: 3261300,
                    sha256: "3aba0be842bd4c192659c781e6fdce8eb9b2b1fff29a40d22f75da369d59f46f",
                },
                File {
                    path: "models/en-tr/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/462a6fc1-0f2d-4b61-ae3f-147bd9f9d588.spm",
                    size: 798793,
                    sha256: "a220363d60391f7ce3df8ec9785424dbc00c24d5786d1a4308cf8ca3ed3385c2",
                },
            ],
        },
        Pair {
            id: "es-en",
            src: "es",
            dst: "en",
            files: [
                File {
                    path: "models/es-en/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/9ee26e91-9b52-44ba-8d30-c0230dd587b2.bin",
                    size: 17140755,
                    sha256: "4b6b7f451094aaa447d012658af158ffc708fc8842dde2f871a58404f5457fe0",
                },
                File {
                    path: "models/es-en/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/54c5d8e2-3900-4a31-860e-8a18f7d028b5.bin",
                    size: 3860888,
                    sha256: "f11a2c23ef85ab1fee1c412b908d69bc20d66fd59faa8f7da5a5f0347eddf969",
                },
                File {
                    path: "models/es-en/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/cab5e093-7b55-47ea-a247-9747cc0109e3.spm",
                    size: 825463,
                    sha256: "909b1eea1face0d7f90a474fe29a8c0fef8d104b6e41e65616f864c964ba8845",
                },
            ],
        },
        Pair {
            id: "fr-en",
            src: "fr",
            dst: "en",
            files: [
                File {
                    path: "models/fr-en/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/e8e44aae-f20c-48c7-89c8-dade397a5858.bin",
                    size: 17140961,
                    sha256: "185f76d24c2d400fe4ea0cb2487df77722641b97a3ef10633872e8a7fdf40e09",
                },
                File {
                    path: "models/fr-en/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/75287f2c-8f15-4045-a435-ec1b9394d973.bin",
                    size: 8818768,
                    sha256: "3148abf21ea98a4d69d0e4504e0d68a6c060204a9b9a39b76855aee1d5b2c8ea",
                },
                File {
                    path: "models/fr-en/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/3096e2ad-89fe-4f9b-aac7-57f011bd4c49.spm",
                    size: 831382,
                    sha256: "4c84b95b62c930f0791466d73eb996841eef474c96d0c2f0e6c6d80640f2005a",
                },
            ],
        },
        Pair {
            id: "it-en",
            src: "it",
            dst: "en",
            files: [
                File {
                    path: "models/it-en/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/1cf6a2f6-f430-4119-98d5-1ec0617beb0f.bin",
                    size: 17140899,
                    sha256: "7dfdf189146d9353fdea264b9e4c8ac36441c770dc4353a8380b64e589dc035b",
                },
                File {
                    path: "models/it-en/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/b38e032b-1b1a-4899-a0e5-80ef69a06814.bin",
                    size: 4977500,
                    sha256: "e30ec549bd0da9ac42cccdcd3806d3be84d485f7fd329f90f6e40ee027e841d9",
                },
                File {
                    path: "models/it-en/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/fbc18b06-35c1-47fa-b2db-5b792366471b.spm",
                    size: 812781,
                    sha256: "603f3349657c3deb9736a0c567452d102a5a03c377dfdf1d32c428608f2cff1b",
                },
            ],
        },
        Pair {
            id: "nl-en",
            src: "nl",
            dst: "en",
            files: [
                File {
                    path: "models/nl-en/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/1db60932-8656-41a8-a00f-191eb925cef8.bin",
                    size: 17140899,
                    sha256: "187f9c8f1ffbcc8ecf0724b608ffed9f15bde262b9de5c4754f8d763b585969d",
                },
                File {
                    path: "models/nl-en/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/eeba1c98-e44a-4d97-8b5c-44c0a56edc3c.bin",
                    size: 4940304,
                    sha256: "15c473e6e5aef1fa3fe5cc804844e19a2c671d2e39169dcbc06f456ca4f9b2c0",
                },
                File {
                    path: "models/nl-en/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/e16e52fe-1c21-48ab-9b7f-af1183a99d1a.spm",
                    size: 807730,
                    sha256: "48a6331cb8a69d21dd24c6c2a15de44a07bcfd4a4470eaf4616849d77945f6b9",
                },
            ],
        },
        Pair {
            id: "pl-en",
            src: "pl",
            dst: "en",
            files: [
                File {
                    path: "models/pl-en/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/b26a428b-a136-469d-91dd-9389f72c053e.bin",
                    size: 17140899,
                    sha256: "172a5f1d44bf8dd6a6eec3868b13b33ce265f3530e898fe11a80b739b821726e",
                },
                File {
                    path: "models/pl-en/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/ce258bcd-2f27-42c4-8f46-4ccaac1158ae.bin",
                    size: 4898024,
                    sha256: "863afade0ba058fb0173fedef3d1fb14d0dcabc24c3b4584cb1fed8f84d6d879",
                },
                File {
                    path: "models/pl-en/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/98b6b745-b695-4543-95d8-adf1996095ce.spm",
                    size: 822587,
                    sha256: "a1d27e6f5c0d29f406364ebf0382949d1c0affc750cec4380f3173150552f43e",
                },
            ],
        },
        Pair {
            id: "pt-en",
            src: "pt",
            dst: "en",
            files: [
                File {
                    path: "models/pt-en/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/dc4327ec-9ebc-4c12-8037-48cd30f3076d.bin",
                    size: 17140899,
                    sha256: "b4a1fd101f59ca258bfb051a3a4e5762497b9f91d7e74f28b7a2d68dc075f8fa",
                },
                File {
                    path: "models/pt-en/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/013e0ebf-3d6b-4723-b83e-0e00ed29477f.bin",
                    size: 4801740,
                    sha256: "702d9b09fbc020168998bb81afc927e74262f5a1455e9b443608a5d8efa4b52d",
                },
                File {
                    path: "models/pt-en/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/75fa56af-540e-4a56-8a9f-1317ae7a9c61.spm",
                    size: 817234,
                    sha256: "2326a27577b5ffa0b822c55fe850986961728097521de35fcc186f7d6dce72d4",
                },
            ],
        },
        Pair {
            id: "ru-en",
            src: "ru",
            dst: "en",
            files: [
                File {
                    path: "models/ru-en/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/b7dbeaa4-6524-424f-8c0b-7784e2b209e5.bin",
                    size: 17140836,
                    sha256: "3b6a0305e3d232fadd54f5a765365b7b96ad6d8f2e818cba594b02fbd8fadb3d",
                },
                File {
                    path: "models/ru-en/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/cc7768c2-ff5f-4dc6-a7c8-82088e36a378.bin",
                    size: 5090836,
                    sha256: "e6667e22f5f86be4872e3768b7184727f5dd8c9f2ccfb0639baabcb1176f5d11",
                },
                File {
                    path: "models/ru-en/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/f450af1b-d1d6-49e4-a7a9-c2e325d80933.spm",
                    size: 936576,
                    sha256: "aaf9a325c0a988c507d0312cb6ba1a02bac7a370bcd879aedee626a40bfbda78",
                },
            ],
        },
        Pair {
            id: "tr-en",
            src: "tr",
            dst: "en",
            files: [
                File {
                    path: "models/tr-en/model.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/553d0a23-6940-49f5-8b2e-155b08897db7.bin",
                    size: 17140836,
                    sha256: "bd18594ac5a7f1d9997e7ea5bd80272082219cf8b1ce604766e4f207eb86abbf",
                },
                File {
                    path: "models/tr-en/lex.bin",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/0c8e7a90-9172-4987-aab2-ed027e0ff3aa.bin",
                    size: 4662492,
                    sha256: "d3374ab5267a73ee1aa1d926a298bd349426835f40856991ee959bc5cd4f9fce",
                },
                File {
                    path: "models/tr-en/vocab.spm",
                    url: "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/b99302c1-af69-4781-930f-d8dec82b8ec9.spm",
                    size: 811353,
                    sha256: "ed328e589a3ccd70fd3ce7773fc3c01d8b7b18c687464cacf17fd40a8c0daadd",
                },
            ],
        },
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    fn all_files() -> Vec<&'static File> {
        let mut files: Vec<&File> = MANIFEST.engine.iter().collect();
        for pair in MANIFEST.pairs {
            files.extend(pair.files.iter());
        }
        files
    }

    #[test]
    fn every_entry_is_pinned_with_a_hash_a_size_and_an_https_url_on_an_allowed_host() {
        let files = all_files();
        assert!(files.len() >= 2 + 3 * 2, "the engine, and English with one other language both ways");
        for file in files {
            assert_eq!(file.sha256.len(), 64, "{}", file.path);
            assert!(file.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{}", file.path);
            assert!(file.size > 0 && file.size <= 40 * 1024 * 1024, "{}: {}", file.path, file.size);
            let url = url::Url::parse(file.url).unwrap_or_else(|e| panic!("{}: {e}", file.url));
            assert_eq!(url.scheme(), "https", "{}", file.url);
            assert!(
                matches!(url.host_str(), Some("firefox-settings-attachments.cdn.mozilla.net" | "github.com")),
                "{}",
                file.url
            );
            assert!(url.query().is_none() && url.fragment().is_none() && url.username().is_empty(), "{}", file.url);
            assert!(!file.path.contains("..") && !file.path.starts_with('/') && !file.path.contains('\\'), "{}", file.path);
        }
    }

    #[test]
    fn paths_are_unique_and_only_the_known_names() {
        let files = all_files();
        let mut paths: Vec<&str> = files.iter().map(|file| file.path).collect();
        paths.sort_unstable();
        let before = paths.len();
        paths.dedup();
        assert_eq!(paths.len(), before, "no two entries share a place on disk");
        for file in files {
            let known = file.path == "engine/bergamot-translator-worker.js"
                || file.path == "engine/bergamot-translator-worker.wasm"
                || (file.path.starts_with("models/")
                    && file.path.matches('/').count() == 2
                    && ["/model.bin", "/lex.bin", "/vocab.spm"].iter().any(|name| file.path.ends_with(name)));
            assert!(known, "{}", file.path);
        }
    }

    #[test]
    fn pairs_are_well_formed_and_ordered_model_shortlist_vocabulary() {
        for pair in MANIFEST.pairs {
            assert_eq!(pair.id, format!("{}-{}", pair.src, pair.dst));
            assert_ne!(pair.src, pair.dst);
            assert!(pair.src == "en" || pair.dst == "en", "{}: English is on one side of every pair", pair.id);
            for code in [pair.src, pair.dst] {
                assert_eq!(crate::translate::normalize_lang(code).as_deref(), Some(code));
            }
            let names: Vec<&str> = pair.files.iter().map(|file| file.path.rsplit('/').next().unwrap()).collect();
            assert_eq!(names, ["model.bin", "lex.bin", "vocab.spm"], "{}", pair.id);
            assert!(pair.files.iter().all(|file| file.path.starts_with(&format!("models/{}/", pair.id))), "{}", pair.id);
        }
        let mut ids: Vec<&str> = MANIFEST.pairs.iter().map(|pair| pair.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    #[test]
    fn english_and_other_languages_are_pinned_both_ways() {
        assert!(MANIFEST.pair("fr", "en").is_some() && MANIFEST.pair("en", "fr").is_some());
        assert!(MANIFEST.pair("de", "en").is_some() && MANIFEST.pair("en", "de").is_some());
        let languages = MANIFEST.languages();
        assert_eq!(languages[0], "en");
        assert!(languages.len() >= 3, "{languages:?}");
    }

    #[test]
    fn the_engine_entries_are_the_two_files_the_worker_page_loads_from_the_pinned_tag() {
        let paths: Vec<&str> = MANIFEST.engine.iter().map(|file| file.path).collect();
        assert_eq!(paths, ["engine/bergamot-translator-worker.js", "engine/bergamot-translator-worker.wasm"]);
        assert!(MANIFEST.engine.iter().all(|file| file.url.contains("/browsermt/bergamot-translator/releases/download/v0.4.5/")));
        assert!(!MANIFEST.engine.iter().any(|file| file.url.contains("/latest/")), "never a moving tag");
    }
}
