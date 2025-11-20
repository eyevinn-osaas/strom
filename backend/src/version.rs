/// Version and build information embedded at compile time
use serde::Serialize;
use utoipa::ToSchema;

/// Build and version information
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct VersionInfo {
    /// Package version from Cargo.toml
    pub version: &'static str,
    /// Git commit hash (short)
    pub git_hash: &'static str,
    /// Git tag (if on a tagged commit)
    pub git_tag: &'static str,
    /// Git branch name
    pub git_branch: &'static str,
    /// Whether the working directory had uncommitted changes
    pub git_dirty: bool,
    /// Build timestamp (ISO 8601 format)
    pub build_timestamp: &'static str,
}

impl VersionInfo {
    /// Get the current version information
    pub fn get() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            git_hash: env!("GIT_HASH"),
            git_tag: env!("GIT_TAG"),
            git_branch: env!("GIT_BRANCH"),
            git_dirty: env!("GIT_DIRTY") == "true",
            build_timestamp: env!("BUILD_TIMESTAMP"),
        }
    }

    /// Get a human-readable version string
    ///
    /// Returns:
    /// - "v0.1.0" if on a tagged release
    /// - "v0.1.0-dev+abc12345" if on main/master without tag
    /// - "v0.1.0-dev+abc12345-dirty" if there are uncommitted changes
    pub fn version_string(&self) -> String {
        if !self.git_tag.is_empty() {
            // On a tagged release
            self.git_tag.to_string()
        } else {
            // Development version
            let mut version = format!("v{}-dev+{}", self.version, self.git_hash);
            if self.git_dirty {
                version.push_str("-dirty");
            }
            version
        }
    }

    /// Get a short version string for display
    ///
    /// Returns:
    /// - "v0.1.0" if on a tagged release
    /// - "v0.1.0-dev" if not on a tag
    pub fn short_version(&self) -> String {
        if !self.git_tag.is_empty() {
            self.git_tag.to_string()
        } else {
            format!("v{}-dev", self.version)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_info() {
        let info = VersionInfo::get();

        // These should always be set by build.rs
        assert!(!info.version.is_empty());
        assert!(!info.git_hash.is_empty());
        assert!(!info.build_timestamp.is_empty());

        // These might be empty depending on git state
        // but shouldn't panic
        let _ = info.version_string();
        let _ = info.short_version();
    }
}
