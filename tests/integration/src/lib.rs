// D-Bus integration tests for SlateOS daemons.
//
// These tests require a running D-Bus session bus. Run via:
//   dbus-run-session cargo test -p slate-integration-tests
//
// On CI or dev machines without a session bus, these tests will
// be skipped (each test checks for D-Bus availability first).

pub mod harness;

#[cfg(test)]
mod notifyd;
#[cfg(test)]
mod rhea;
#[cfg(test)]
mod cross_daemon;
