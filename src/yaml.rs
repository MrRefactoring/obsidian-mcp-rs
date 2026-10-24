use serde::de::DeserializeOwned;

fn config() -> noyalib::ParserConfig {
    noyalib::ParserConfig::serde_yaml_compat()
        .duplicate_key_policy(noyalib::DuplicateKeyPolicy::Error)
}

pub(crate) fn parse<T: DeserializeOwned + 'static>(input: &str) -> Result<T, noyalib::Error> {
    noyalib::from_str_with_config(input, &config())
}

pub(crate) fn document(input: &str) -> Result<noyalib::cst::Document, noyalib::Error> {
    noyalib::cst::parse_document_with_config(input, &config())
}
