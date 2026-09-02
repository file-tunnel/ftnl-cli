//! Bounded File Tunnel CLI events for an application-owned next-loggers transport.
//!
//! This module deliberately cannot accept argv, error strings, capabilities,
//! pairing fragments, event tickets, filenames, metadata, local paths, or file
//! content. A caller may adapt [`NextLoggersTransport`] to `ores.otel.log`,
//! OpenTelemetry, or another application-owned provider without giving the
//! telemetry layer access to File Tunnel secrets.

use serde::Serialize;

/// The wire schema implemented by `ores-otel/ores.otel.log`.
pub const NEXT_LOGGERS_SCHEMA: &str = "next-loggers/v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TelemetryLevel {
    Info,
    Warn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CliPhase {
    Started,
    Finished,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CliOutcome {
    Success,
    Failure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitClass {
    Success,
    Usage,
    Failure,
}

impl ExitClass {
    pub fn from_exit_code(exit_code: i32) -> Self {
        match exit_code {
            0 => Self::Success,
            2 => Self::Usage,
            _ => Self::Failure,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliTelemetryFields {
    pub component: &'static str,
    pub phase: CliPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<CliOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_class: Option<ExitClass>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliTelemetryEvent {
    pub schema: &'static str,
    pub app_name: &'static str,
    pub runtime: &'static str,
    pub level: TelemetryLevel,
    pub message: &'static str,
    pub fields: CliTelemetryFields,
}

impl CliTelemetryEvent {
    fn started() -> Self {
        Self {
            schema: NEXT_LOGGERS_SCHEMA,
            app_name: "ftnl-cli",
            runtime: "rust-cli",
            level: TelemetryLevel::Info,
            message: "file_tunnel_cli_invocation_started",
            fields: CliTelemetryFields {
                component: "cli",
                phase: CliPhase::Started,
                outcome: None,
                exit_class: None,
            },
        }
    }

    fn finished(exit_code: i32) -> Self {
        let exit_class = ExitClass::from_exit_code(exit_code);
        let outcome = if exit_class == ExitClass::Success {
            CliOutcome::Success
        } else {
            CliOutcome::Failure
        };
        Self {
            schema: NEXT_LOGGERS_SCHEMA,
            app_name: "ftnl-cli",
            runtime: "rust-cli",
            level: if outcome == CliOutcome::Success {
                TelemetryLevel::Info
            } else {
                TelemetryLevel::Warn
            },
            message: "file_tunnel_cli_invocation_finished",
            fields: CliTelemetryFields {
                component: "cli",
                phase: CliPhase::Finished,
                outcome: Some(outcome),
                exit_class: Some(exit_class),
            },
        }
    }
}

/// Explicit transport boundary owned and configured by the application.
///
/// Implementations receive only [`CliTelemetryEvent`], whose fields are closed
/// enums and fixed strings. The transport is responsible for converting the
/// event into the full `next-loggers/v1` record and for installing or selecting
/// any OpenTelemetry provider. This crate never installs a global provider.
pub trait NextLoggersTransport {
    type Error;

    fn write(&self, event: &CliTelemetryEvent) -> Result<(), Self::Error>;

    fn flush(&self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NoopNextLoggersTransport;

impl NextLoggersTransport for NoopNextLoggersTransport {
    type Error = std::convert::Infallible;

    fn write(&self, _event: &CliTelemetryEvent) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub struct CliTelemetry<T> {
    transport: T,
}

impl<T> CliTelemetry<T>
where
    T: NextLoggersTransport,
{
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn invocation_started(&self) -> Result<(), T::Error> {
        self.transport.write(&CliTelemetryEvent::started())
    }

    pub fn invocation_finished(&self, exit_code: i32) -> Result<(), T::Error> {
        self.transport
            .write(&CliTelemetryEvent::finished(exit_code))
    }

    pub fn flush(&self) -> Result<(), T::Error> {
        self.transport.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct CaptureTransport {
        records: Mutex<Vec<String>>,
    }

    impl NextLoggersTransport for CaptureTransport {
        type Error = serde_json::Error;

        fn write(&self, event: &CliTelemetryEvent) -> Result<(), Self::Error> {
            self.records
                .lock()
                .expect("capture transport poisoned")
                .push(serde_json::to_string(event)?);
            Ok(())
        }
    }

    #[test]
    fn lifecycle_records_use_only_the_closed_safe_field_set() {
        let telemetry = CliTelemetry::new(CaptureTransport::default());
        telemetry.invocation_started().expect("start event");
        telemetry.invocation_finished(2).expect("finish event");

        let records = telemetry
            .transport()
            .records
            .lock()
            .expect("capture transport poisoned");
        assert_eq!(records.len(), 2);
        assert!(records[0].contains("\"schema\":\"next-loggers/v1\""));
        assert!(records[0].contains("file_tunnel_cli_invocation_started"));
        assert!(records[1].contains("\"exitClass\":\"usage\""));
        assert!(records[1].contains("\"outcome\":\"failure\""));

        let serialized = records.join("\n").to_ascii_lowercase();
        for forbidden in [
            "capability",
            "pairing",
            "ticket",
            "filename",
            "file_name",
            "local_path",
            "metadata",
            "content",
            "password",
            "token",
            "secret",
            "postgres://",
            "https://",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "sensitive File Tunnel field entered telemetry: {forbidden}"
            );
        }
    }

    #[test]
    fn raw_exit_codes_collapse_to_three_bounded_classes() {
        assert_eq!(ExitClass::from_exit_code(0), ExitClass::Success);
        assert_eq!(ExitClass::from_exit_code(2), ExitClass::Usage);
        assert_eq!(ExitClass::from_exit_code(1), ExitClass::Failure);
        assert_eq!(ExitClass::from_exit_code(127), ExitClass::Failure);
        assert_eq!(ExitClass::from_exit_code(-1), ExitClass::Failure);
    }
}
