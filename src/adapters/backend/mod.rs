#[cfg(test)]
#[path = "fake_remote_fixture_tests.rs"]
pub(crate) mod fake_remote;
pub mod local;
pub mod remote;
pub mod smb;

#[doc(hidden)]
pub mod fake;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
