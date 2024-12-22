use super::*;

#[test]
fn test_piscem_known_chems() {
    let idx_type = IndexType::Piscem(PathBuf::new());
    let custom_chem_p = PathBuf::from("resources").join("chemistries.json");
    let chem = "10xv2";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(c, Chemistry::Rna(RnaChemistry::TenxV2));
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "10xv3";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(c, Chemistry::Rna(RnaChemistry::TenxV3));
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "10xv4-3p";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(c, Chemistry::Rna(RnaChemistry::TenxV43P));
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "10xv2-5p";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(c, Chemistry::Rna(RnaChemistry::TenxV25P));
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "10xv3-5p";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(c, Chemistry::Rna(RnaChemistry::TenxV35P));
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);
}

#[test]
fn test_salmon_known_chems() {
    let idx_type = IndexType::Salmon(PathBuf::new());
    let custom_chem_p = PathBuf::from("resources").join("chemistries.json");

    let chem = "10xv2";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(c, Chemistry::Rna(RnaChemistry::TenxV2));
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "10xv3";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(c, Chemistry::Rna(RnaChemistry::TenxV3));
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "10xv4-3p";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(c, Chemistry::Rna(RnaChemistry::TenxV43P));
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "dropseq";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(
        c,
        Chemistry::Rna(RnaChemistry::Other(String::from("dropseq")))
    );
    assert_eq!(c.expected_ori(), ExpectedOri::Both);

    let chem = "indropv2";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(
        c,
        Chemistry::Rna(RnaChemistry::Other(String::from("indropv2")))
    );
    assert_eq!(c.expected_ori(), ExpectedOri::Both);

    let chem = "citeseq";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(
        c,
        Chemistry::Rna(RnaChemistry::Other(String::from("citeseq")))
    );
    assert_eq!(c.expected_ori(), ExpectedOri::Both);

    let chem = "celseq2";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(
        c,
        Chemistry::Rna(RnaChemistry::Other(String::from("celseq2")))
    );
    assert_eq!(c.expected_ori(), ExpectedOri::Both);

    /*
    "splitseqv1" => "--splitseqV1",
    "splitseqv2" => "--splitseqV2",
    "sciseq3" => "--sciseq3"
    */
}
