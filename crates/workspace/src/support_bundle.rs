//! A finite, metadata-only support bundle. It never opens project files or
//! accepts free-form diagnostic text, paths, mail, logs, or secrets.
//!
//! The caller must authorize the project and retain these exact bytes for both
//! preview and any separately reviewed local export.

use serde::Serialize;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Serialize)]
struct Bundle {
    schema_version: u8,
    doctor: Doctor,
    runbook: Runbook,
    omitted: [&'static str; 5],
}

#[derive(Serialize)]
struct Doctor {
    code: &'static str,
    origin: &'static str,
    observed_at: u64,
    revision: u64,
}

#[derive(Serialize)]
struct Runbook {
    evidence: &'static str,
    impact: &'static str,
    next_action: Option<&'static str>,
}

/// Project-specific freshness and an existing doctor code are the only inputs.
/// Unknown codes are rejected instead of inserting arbitrary diagnostic text.
pub fn render(doctor_code: &str, observed_at: u64, revision: u64) -> Option<Vec<u8>> {
    if observed_at > MAX_SAFE_INTEGER || revision > MAX_SAFE_INTEGER {
        return None;
    }
    let (code, evidence, impact, next_action) = match doctor_code {
        "cached_metadata_unavailable" => (
            "cached_metadata_unavailable",
            "snapshot_unavailable",
            "diagnosis_incomplete",
            Some("health.doctor"),
        ),
        "cached_metadata_stale" => (
            "cached_metadata_stale",
            "revision_mismatch",
            "diagnosis_incomplete",
            Some("health.doctor"),
        ),
        "runtime_missing" => (
            "runtime_missing",
            "runtime_not_confirmed_in_snapshot",
            "runtime_may_be_unavailable",
            Some("runtimes.install"),
        ),
        "service_failed" => (
            "service_failed",
            "cached_service_failed",
            "project_service_unavailable",
            Some("services.status"),
        ),
        "service_stopped" => (
            "service_stopped",
            "cached_service_stopped",
            "project_service_unavailable",
            Some("services.start"),
        ),
        "cached_metadata_available" => (
            "cached_metadata_available",
            "cached_project_snapshot",
            "no_known_issue",
            None,
        ),
        _ => return None,
    };
    let bundle = Bundle {
        schema_version: 1,
        doctor: Doctor {
            code,
            origin: "cached",
            observed_at,
            revision,
        },
        runbook: Runbook {
            evidence,
            impact,
            next_action,
        },
        omitted: [
            "environment_values",
            "private_keys",
            "database_dumps",
            "logs_and_mail_content",
            "arbitrary_files",
        ],
    };
    // Serialization of these finite strings and integers is infallible.
    serde_json::to_vec(&bundle)
        .ok()
        .filter(|bytes| bytes.len() <= 4096)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_bounded_deterministic_and_contains_only_allowlisted_metadata() {
        for code in [
            "cached_metadata_unavailable",
            "cached_metadata_stale",
            "runtime_missing",
            "service_failed",
            "service_stopped",
            "cached_metadata_available",
        ] {
            let first = render(code, 123, 4).unwrap();
            assert!(first.len() <= 4096);
            assert_eq!(first, render(code, 123, 4).unwrap());
            let value: serde_json::Value = serde_json::from_slice(&first).unwrap();
            assert_eq!(value["doctor"]["code"], code);
            assert_eq!(value["doctor"]["origin"], "cached");
            assert_eq!(
                value["omitted"],
                serde_json::json!([
                    "environment_values",
                    "private_keys",
                    "database_dumps",
                    "logs_and_mail_content",
                    "arbitrary_files"
                ])
            );
        }
        assert!(render("TOKEN=secret-canary", 123, 4).is_none());
        assert!(render("service_failed", MAX_SAFE_INTEGER + 1, 4).is_none());
        assert!(render("service_failed", 123, MAX_SAFE_INTEGER + 1).is_none());
    }
}
