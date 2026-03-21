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
fn test_no_index_known_chems() {
    let idx_type = IndexType::NoIndex;
    let custom_chem_p = PathBuf::from("resources").join("chemistries.json");
    let cs = [
        "10xv2",
        "10xv3",
        "10xv4-3p",
        "10xv2-5p",
        "10xv3-5p",
    ];

    let dirs = vec![ExpectedOri::Forward; cs.len()];

    for (chem, dir) in cs.iter().zip(dirs.iter()) {
        let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem);
        match c {
            Err(e) => panic!(
                "Couldn't lookup {} for the no-index mapper, but it should succeed :: {:#}",
                chem, e
            ),
            Ok(c) => {
                println!("testing ori for {}", chem);
                assert_eq!(&c.expected_ori(), dir);
            }
        }
    }
}

#[test]
fn test_invalid_chemistry_name() {
    let piscem_idx = IndexType::Piscem(PathBuf::new());
    let custom_chem_p = PathBuf::from("resources").join("chemistries.json");

    let indexes = vec![piscem_idx, IndexType::NoIndex];
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
fn test_custom_general_geometry_is_accepted_for_piscem() {
    let idx_type = IndexType::Piscem(PathBuf::new());
    let custom_chem_p = PathBuf::from("resources").join("chemistries.json");
    let chem = r#"1{b[16]u[12]x[0-3]hamming(f[TTGCTAGGACCG],1)s[10]x:}2{r:}"#;

    let c = Chemistry::from_str(&idx_type, &custom_chem_p, chem)
        .expect("general geometry should be accepted");
    match c {
        Chemistry::Custom(custom) => assert_eq!(custom.geometry(), chem),
        other => panic!("expected custom chemistry, found {:?}", other),
    }
}
