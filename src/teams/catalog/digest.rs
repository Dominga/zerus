//! Version 1 fingerprints the resolved, pinned bundle. Object keys are sorted;
//! array order and every serialized bundle field are retained. Clients keep
//! this value opaque and never compute it themselves.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zerus_team_plan::{problem::code, Problem, TemplateBundle};

pub(crate) fn template_digest(bundle: &TemplateBundle) -> Result<String, Problem> {
    Ok(format!("{:x}", Sha256::digest(canonical_bytes(bundle)?)))
}
fn canonical_bytes(bundle: &TemplateBundle) -> Result<Vec<u8>, Problem> {
    let value = json!({"schema":1,"bundle":bundle});
    let mut out = Vec::new();
    canonical(&value, &mut out)?;
    out.push(b'\n');
    Ok(out)
}
fn canonical(value: &Value, out: &mut Vec<u8>) -> Result<(), Problem> {
    match value {
        Value::Object(fields) => {
            out.push(b'{');
            let mut keys = fields.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                serde_json::to_writer(&mut *out, key).map_err(encoding)?;
                out.push(b':');
                canonical(&fields[key], out)?;
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                canonical(item, out)?;
            }
            out.push(b']');
        }
        _ => serde_json::to_writer(out, value).map_err(encoding)?,
    }
    Ok(())
}
fn encoding(error: impl std::fmt::Display) -> Problem {
    Problem::new(
        code::INVALID_STATE,
        format!("The template cannot be encoded: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> TemplateBundle {
        zerus_team_plan::bundle(&zerus_team_plan::builtin(), "bugfix").unwrap()
    }
    #[test]
    fn the_create_encoding_and_sha256_are_pinned() {
        let bundle = sample();
        assert_eq!(
            canonical_bytes(&bundle).unwrap(),
            include_bytes!("../../../crates/team-plan/fixtures/template-create-canonical.json")
        );
        assert_eq!(
            template_digest(&bundle).unwrap(),
            include_str!("../../../crates/team-plan/fixtures/template-create-digest.txt").trim()
        );
    }
    #[test]
    #[ignore = "rewrites the versioned canonical encoding only for an intentional contract change"]
    fn regenerate_canonical_fixture() {
        let bundle = sample();
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/team-plan/fixtures");
        std::fs::write(
            root.join("template-create-canonical.json"),
            canonical_bytes(&bundle).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("template-create-digest.txt"),
            format!("{}\n", template_digest(&bundle).unwrap()),
        )
        .unwrap();
    }
}
