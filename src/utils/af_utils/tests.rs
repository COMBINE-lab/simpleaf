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
fn test_invalid_chemistry_name() {
    let piscem_idx = IndexType::Piscem(PathBuf::new());
    let salmon_idx = IndexType::Salmon(PathBuf::new());
    let custom_chem_p = PathBuf::from("resources").join("chemistries.json");

    let indexes = vec![piscem_idx, salmon_idx];
    let cs = vec!["flerb"];
    for idx_type in indexes {
        for chem in &cs {
            let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem);
            match c {
                Err(_) => (),
                Ok(_) => panic!(
                    "This lookup of chemistry {} in the piscem mapper should not succeed!",
                    chem
                ),
            }
        }
    }
}

#[test]
fn test_piscem_known_invalid() {
    let piscem_idx = IndexType::Piscem(PathBuf::new());
    let custom_chem_p = PathBuf::from("resources").join("chemistries.json");
    let cs = vec![
        // forward
        "splitseqv1",
        "splitseqv2",
        "indropv2",
        "sciseq3",
        "dropseq",
        "citeseq",
        // reverse
        "celseq2",
    ];

    for chem in &cs {
        let c = Chemistry::from_str(&piscem_idx, &custom_chem_p, chem);
        match c {
            Err(e) => println!("{:?}", e),
            Ok(_) => panic!(
                "This lookup of chemistry {} in the piscem mapper should not succeed!",
                chem
            ),
        }
    }
}

#[test]
fn test_salmon_known_invalid() {
    let salmon_idx = IndexType::Salmon(PathBuf::new());
    let custom_chem_p = PathBuf::from("resources").join("chemistries.json");
    let cs = vec!["10xv3-5p", "10xv2-5p"];

    for chem in &cs {
        let c = Chemistry::from_str(&salmon_idx, &custom_chem_p, chem);
        match c {
            Err(e) => println!("{:?}", e),
            Ok(_) => panic!(
                "This lookup of chemistry {} in the piscem mapper should not succeed!",
                chem
            ),
        }
    }
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
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "citeseq";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(
        c,
        Chemistry::Rna(RnaChemistry::Other(String::from("citeseq")))
    );
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "celseq2";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(
        c,
        Chemistry::Rna(RnaChemistry::Other(String::from("celseq2")))
    );
    assert_eq!(c.expected_ori(), ExpectedOri::Reverse);

    let chem = "splitseqv1";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(
        c,
        Chemistry::Rna(RnaChemistry::Other(String::from("splitseqv1")))
    );
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "splitseqv2";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(
        c,
        Chemistry::Rna(RnaChemistry::Other(String::from("splitseqv2")))
    );
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);

    let chem = "sciseq3";
    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("should be able to obtain chemistry");
    assert_eq!(
        c,
        Chemistry::Rna(RnaChemistry::Other(String::from("sciseq3")))
    );
    assert_eq!(c.expected_ori(), ExpectedOri::Forward);
}
