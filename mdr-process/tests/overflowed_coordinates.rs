//! `mdr-process sequence` on a PDB with an overflowed coordinate
//! (`********`, mdrepo-rs#89): rejected, with the line, columns and why.
//! The largest values that fit still read.

use anyhow::Result;
use assert_cmd::cargo_bin_cmd;
use std::fs;

/// CA beads MET 1, ALA 2, LYS 3 with bead 2's z field `z2`
fn beads(z2: &str) -> String {
    format!(
        "ATOM      1  CA  MET A   1       0.000   0.000   0.000  1.00  0.00           C  \n\
         ATOM      2  CA  ALA A   2       3.800   0.000{z2:>8}  1.00  0.00           C  \n\
         ATOM      3  CA  LYS A   3       7.600   0.000   0.000  1.00  0.00           C  \n\
         END\n"
    )
}

fn sequence_of(pdb: &str) -> Result<String> {
    let d = tempfile::tempdir()?;
    let p = d.path().join("x.pdb");
    fs::write(&p, pdb)?;
    let out = cargo_bin_cmd!().arg("sequence").arg(&p).output()?;
    Ok(String::from_utf8(out.stdout)?)
}

#[test]
fn an_overflowed_coordinate_is_rejected_with_why() -> Result<()> {
    let out = sequence_of(&beads("********"))?;
    assert!(out.contains("\tERROR\t"), "{out}");
    assert!(
        out.contains("line 2: columns 47-54: an overflowed coordinate"),
        "{out}"
    );
    Ok(())
}

#[test]
fn a_malformed_coordinate_is_reported_with_its_line() -> Result<()> {
    let out = sequence_of(&beads("12.3ab"))?;
    assert!(out.contains("\tERROR\t"), "{out}");
    assert!(out.contains("line 2: columns 47-54:"), "{out}");
    assert!(!out.contains("overflowed"), "{out}");
    Ok(())
}

#[test]
fn a_clean_file_gives_the_sequence() -> Result<()> {
    let out = sequence_of(&beads("0.000"))?;
    assert!(out.contains("\tMAK\t"), "{out}");
    assert!(!out.contains("ERROR"), "{out}");
    Ok(())
}
