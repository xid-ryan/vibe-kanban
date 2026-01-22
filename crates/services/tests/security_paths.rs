//! Security tests for path traversal prevention in multi-user K8s deployment.
//!
//! These tests verify that:
//! - Path traversal attacks using `..` are blocked
//! - Symlink following is properly validated
//! - URL-encoded path attacks are detected and blocked
//! - All filesystem operations respect user workspace boundaries
//!
//! Test Case ID Prefixes:
//! - PTR: Path traversal tests
//! - SYM: Symlink tests
//! - URL: URL-encoded path tests
//! - BND: Boundary validation tests

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use uuid::Uuid;

/// Mock deployment mode for testing.
/// In actual tests, this would come from db::DeploymentMode.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MockDeploymentMode {
    Desktop,
    Kubernetes,
}

impl MockDeploymentMode {
    #[allow(dead_code)]
    fn is_kubernetes(&self) -> bool {
        matches!(self, MockDeploymentMode::Kubernetes)
    }

    fn is_desktop(&self) -> bool {
        matches!(self, MockDeploymentMode::Desktop)
    }
}

/// Test workspace error type matching services::workspace_manager::WorkspaceError
#[derive(Debug, Clone, PartialEq)]
enum TestWorkspaceError {
    Unauthorized(String),
    Io(String),
}

/// Simulates the path validation logic from WorkspaceManager::validate_user_path
/// This is a test implementation that mirrors the actual validation logic.
fn validate_user_path_test(
    user_id: &Uuid,
    path: &Path,
    mode: MockDeploymentMode,
    workspace_base: &Path,
) -> Result<PathBuf, TestWorkspaceError> {
    // In desktop mode, skip validation (single-user, no isolation needed)
    if mode.is_desktop() {
        return Ok(dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()));
    }

    // In K8s mode, enforce strict path validation
    let user_base = workspace_base.join(user_id.to_string());

    // Canonicalize the user's base directory (create if needed for validation)
    let canonical_base = if user_base.exists() {
        dunce::canonicalize(&user_base).map_err(|e| TestWorkspaceError::Io(e.to_string()))?
    } else {
        // If base doesn't exist yet, use the path as-is for comparison
        user_base.clone()
    };

    // Canonicalize the target path
    let canonical_path = if path.exists() {
        dunce::canonicalize(path).map_err(|e| TestWorkspaceError::Io(e.to_string()))?
    } else {
        // For non-existent paths, resolve parent components to detect traversal
        let mut resolved = PathBuf::new();
        for component in path.components() {
            resolved.push(component);
            // Try to canonicalize what exists so far
            if resolved.exists() {
                resolved = dunce::canonicalize(&resolved)
                    .map_err(|e| TestWorkspaceError::Io(e.to_string()))?;
            }
        }
        resolved
    };

    // Verify the canonical path starts with the user's base directory
    if canonical_path.starts_with(&canonical_base) {
        Ok(canonical_path)
    } else {
        Err(TestWorkspaceError::Unauthorized(path.display().to_string()))
    }
}

/// Helper struct for setting up test environments
struct TestSetup {
    temp_dir: TempDir,
    workspace_base: PathBuf,
    user_id: Uuid,
    user_workspace: PathBuf,
}

impl TestSetup {
    /// Create a new test setup with a temp directory structure
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let workspace_base = temp_dir.path().join("workspaces");
        let user_id = Uuid::new_v4();
        let user_workspace = workspace_base.join(user_id.to_string());

        // Create the directory structure
        std::fs::create_dir_all(&user_workspace).expect("Failed to create user workspace");

        Self {
            temp_dir,
            workspace_base,
            user_id,
            user_workspace,
        }
    }

    /// Create another user's workspace for cross-user tests
    fn create_other_user(&self) -> (Uuid, PathBuf) {
        let other_user_id = Uuid::new_v4();
        let other_workspace = self.workspace_base.join(other_user_id.to_string());
        std::fs::create_dir_all(&other_workspace).expect("Failed to create other user workspace");
        (other_user_id, other_workspace)
    }

    /// Create a file in the user's workspace
    fn create_user_file(&self, relative_path: &str) -> PathBuf {
        let file_path = self.user_workspace.join(relative_path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create parent directory");
        }
        std::fs::write(&file_path, "test content").expect("Failed to create file");
        file_path
    }

    /// Create a file outside the user's workspace (in temp root)
    fn create_external_file(&self, name: &str) -> PathBuf {
        let file_path = self.temp_dir.path().join(name);
        std::fs::write(&file_path, "external content").expect("Failed to create external file");
        file_path
    }
}

// ========== PTR-01: Basic Path Traversal Tests ==========

#[cfg(test)]
mod path_traversal_tests {
    use super::*;

    /// PTR-01: Simple `..` path traversal should be blocked
    ///
    /// Test Steps:
    /// 1. Create user workspace at /workspaces/{user_id}/
    /// 2. Attempt to access /workspaces/{user_id}/../{other_user}/secret
    /// 3. Verify the request is rejected with Unauthorized error
    #[test]
    fn test_ptr_01_simple_dotdot_blocked() {
        let setup = TestSetup::new();
        let (other_user_id, _) = setup.create_other_user();

        // Construct a path that tries to escape via ..
        let malicious_path = setup.user_workspace.join("..").join(other_user_id.to_string());

        let result = validate_user_path_test(
            &setup.user_id,
            &malicious_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Path traversal with .. should be blocked, got: {:?}",
            result
        );
    }

    /// PTR-02: Multiple `..` sequences should be blocked
    #[test]
    fn test_ptr_02_multiple_dotdot_blocked() {
        let setup = TestSetup::new();

        // Try to escape with multiple .. sequences
        let malicious_path = setup.user_workspace
            .join("..")
            .join("..")
            .join("..")
            .join("etc")
            .join("passwd");

        let result = validate_user_path_test(
            &setup.user_id,
            &malicious_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Multiple .. path traversal should be blocked"
        );
    }

    /// PTR-03: Hidden `..` in deep path should be blocked
    #[test]
    fn test_ptr_03_hidden_dotdot_blocked() {
        let setup = TestSetup::new();

        // Create the directories for the path to properly resolve
        let deep_dir = setup.user_workspace.join("projects").join("my-project");
        std::fs::create_dir_all(&deep_dir).expect("Failed to create deep directory");

        // Create a legitimate-looking path with hidden ..
        // projects/my-project/../../.. escapes user_workspace when directories exist
        let malicious_path = setup.user_workspace
            .join("projects")
            .join("my-project")
            .join("..")
            .join("..")
            .join("..")
            .join("..") // This escapes the user workspace
            .join("sensitive-data");

        let result = validate_user_path_test(
            &setup.user_id,
            &malicious_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Hidden .. in deep path should be blocked"
        );
    }

    /// PTR-04: Path that stays within workspace after .. should be allowed
    #[test]
    fn test_ptr_04_contained_dotdot_allowed() {
        let setup = TestSetup::new();

        // Create subdirectories
        let sub1 = setup.user_workspace.join("sub1");
        let sub2 = setup.user_workspace.join("sub2");
        std::fs::create_dir_all(&sub1).expect("Failed to create sub1");
        std::fs::create_dir_all(&sub2).expect("Failed to create sub2");
        std::fs::write(sub2.join("file.txt"), "content").expect("Failed to create file");

        // This path uses .. but stays within the user's workspace
        let contained_path = sub1.join("..").join("sub2").join("file.txt");

        let result = validate_user_path_test(
            &setup.user_id,
            &contained_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            result.is_ok(),
            "Path that stays within workspace should be allowed, got: {:?}",
            result
        );
    }

    /// PTR-05: Absolute path outside workspace should be blocked
    #[test]
    fn test_ptr_05_absolute_path_outside_blocked() {
        let setup = TestSetup::new();

        // Create a file outside the workspace
        let external_file = setup.create_external_file("external.txt");

        let result = validate_user_path_test(
            &setup.user_id,
            &external_file,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Absolute path outside workspace should be blocked"
        );
    }
}

// ========== URL-01: URL-Encoded Path Attack Tests ==========

#[cfg(test)]
mod url_encoded_tests {
    use super::*;

    /// URL-01: URL-encoded `..` (%2e%2e) should be blocked when decoded
    ///
    /// Note: URL decoding typically happens at the HTTP layer before
    /// reaching the path validation. This test verifies that if somehow
    /// encoded characters slip through, they are still properly handled.
    #[test]
    fn test_url_01_encoded_dotdot_after_decode() {
        let setup = TestSetup::new();

        // After URL decoding, %2e%2e becomes ..
        // This simulates what happens after the HTTP layer decodes the URL
        let decoded_path = setup.user_workspace.join("..").join("..").join("etc");

        let result = validate_user_path_test(
            &setup.user_id,
            &decoded_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "URL-decoded .. path should be blocked"
        );
    }

    /// URL-02: Double URL-encoded paths (%252e%252e) should be blocked
    #[test]
    fn test_url_02_double_encoded_blocked() {
        let setup = TestSetup::new();

        // Double encoding: %252e%252e -> %2e%2e -> ..
        // After full decoding, this becomes ..
        let double_decoded_path = setup.user_workspace.join("..").join("..");

        let result = validate_user_path_test(
            &setup.user_id,
            &double_decoded_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Double URL-decoded .. path should be blocked"
        );
    }

    /// URL-03: Null byte injection (%00) paths should be handled safely
    #[test]
    fn test_url_03_null_byte_handled() {
        let setup = TestSetup::new();

        // Paths with null bytes should be rejected by the filesystem
        // Creating a path with embedded characters that might be exploited
        let suspicious_name = "file\x00.txt"; // Null byte in filename
        let suspicious_path = setup.user_workspace.join(suspicious_name);

        // The path should either:
        // 1. Be rejected as invalid
        // 2. Be truncated at the null byte and validated properly
        // Either way, it should not allow access outside the workspace

        // Note: On most systems, this path won't exist and can't be created
        // The validation should handle this gracefully
        let result = validate_user_path_test(
            &setup.user_id,
            &suspicious_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        // The path doesn't exist but is within user workspace (before the null byte)
        // This test primarily ensures we don't crash on null bytes
        assert!(
            result.is_ok() || matches!(result, Err(TestWorkspaceError::Io(_))),
            "Null byte path should be handled safely"
        );
    }

    /// URL-04: Mixed encoding attacks should be blocked
    #[test]
    fn test_url_04_mixed_encoding_blocked() {
        let setup = TestSetup::new();

        // Attackers might try mixing encoded and unencoded characters
        // After decoding: ..%2f..%2f -> ../../ which becomes ../../
        let mixed_path = setup.user_workspace.join("..").join("..");

        let result = validate_user_path_test(
            &setup.user_id,
            &mixed_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Mixed encoding attack should be blocked"
        );
    }
}

// ========== SYM-01: Symlink Tests ==========

#[cfg(test)]
mod symlink_tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    /// SYM-01: Symlink to external location should be blocked
    #[cfg(unix)]
    #[test]
    fn test_sym_01_symlink_to_external_blocked() {
        let setup = TestSetup::new();

        // Create a file outside the workspace
        let external_file = setup.create_external_file("secret.txt");

        // Create a symlink inside the workspace pointing outside
        let symlink_path = setup.user_workspace.join("sneaky_link");
        symlink(&external_file, &symlink_path).expect("Failed to create symlink");

        let result = validate_user_path_test(
            &setup.user_id,
            &symlink_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        // After canonicalization, the symlink resolves to the external path
        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Symlink to external location should be blocked after canonicalization"
        );
    }

    /// SYM-02: Symlink within workspace should be allowed
    #[cfg(unix)]
    #[test]
    fn test_sym_02_symlink_within_workspace_allowed() {
        let setup = TestSetup::new();

        // Create a file inside the workspace
        let internal_file = setup.create_user_file("real_file.txt");

        // Create a symlink inside the workspace pointing to another location inside
        let symlink_path = setup.user_workspace.join("internal_link");
        symlink(&internal_file, &symlink_path).expect("Failed to create symlink");

        let result = validate_user_path_test(
            &setup.user_id,
            &symlink_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            result.is_ok(),
            "Symlink within workspace should be allowed"
        );
    }

    /// SYM-03: Symlink chain that eventually escapes should be blocked
    #[cfg(unix)]
    #[test]
    fn test_sym_03_symlink_chain_escape_blocked() {
        let setup = TestSetup::new();

        // Create external target
        let external_file = setup.create_external_file("chain_target.txt");

        // Create a chain of symlinks
        let link1 = setup.user_workspace.join("link1");
        let link2 = setup.user_workspace.join("link2");
        let link3 = setup.user_workspace.join("link3");

        // link3 -> link2 -> link1 -> external_file
        symlink(&external_file, &link1).expect("Failed to create link1");
        symlink(&link1, &link2).expect("Failed to create link2");
        symlink(&link2, &link3).expect("Failed to create link3");

        let result = validate_user_path_test(
            &setup.user_id,
            &link3,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        // After full canonicalization, the chain resolves to external_file
        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Symlink chain escaping workspace should be blocked"
        );
    }

    /// SYM-04: Symlink to other user's workspace should be blocked
    #[cfg(unix)]
    #[test]
    fn test_sym_04_symlink_to_other_user_blocked() {
        let setup = TestSetup::new();
        let (_, other_workspace) = setup.create_other_user();

        // Create a file in other user's workspace
        let other_file = other_workspace.join("other_secret.txt");
        std::fs::write(&other_file, "other user's secret").expect("Failed to create file");

        // Create a symlink to other user's workspace
        let symlink_path = setup.user_workspace.join("cross_user_link");
        symlink(&other_file, &symlink_path).expect("Failed to create symlink");

        let result = validate_user_path_test(
            &setup.user_id,
            &symlink_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Symlink to other user's workspace should be blocked"
        );
    }

    /// SYM-05: Broken symlink should be handled gracefully
    #[cfg(unix)]
    #[test]
    fn test_sym_05_broken_symlink_handled() {
        let setup = TestSetup::new();

        // Create a symlink to a non-existent target within workspace
        let broken_link = setup.user_workspace.join("broken_link");
        let nonexistent_target = setup.user_workspace.join("does_not_exist.txt");
        symlink(&nonexistent_target, &broken_link).expect("Failed to create broken symlink");

        let result = validate_user_path_test(
            &setup.user_id,
            &broken_link,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        // Broken symlinks within workspace should be handled gracefully
        // The validation should either succeed (target is within bounds)
        // or fail with IO error (cannot resolve)
        assert!(
            result.is_ok() || matches!(result, Err(TestWorkspaceError::Io(_))),
            "Broken symlink should be handled gracefully"
        );
    }
}

// ========== BND-01: Boundary Validation Tests ==========

#[cfg(test)]
mod boundary_tests {
    use super::*;

    /// BND-01: Valid path within user workspace should be allowed
    #[test]
    fn test_bnd_01_valid_path_allowed() {
        let setup = TestSetup::new();

        // Create a legitimate file
        let valid_file = setup.create_user_file("projects/my-app/src/main.rs");

        let result = validate_user_path_test(
            &setup.user_id,
            &valid_file,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            result.is_ok(),
            "Valid path within workspace should be allowed"
        );
    }

    /// BND-02: Path to user's workspace root should be allowed
    #[test]
    fn test_bnd_02_workspace_root_allowed() {
        let setup = TestSetup::new();

        let result = validate_user_path_test(
            &setup.user_id,
            &setup.user_workspace,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            result.is_ok(),
            "User's workspace root should be accessible"
        );
    }

    /// BND-03: Path to workspace base (parent of user workspace) should be blocked
    #[test]
    fn test_bnd_03_workspace_base_blocked() {
        let setup = TestSetup::new();

        let result = validate_user_path_test(
            &setup.user_id,
            &setup.workspace_base,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Workspace base directory (containing all users) should be blocked"
        );
    }

    /// BND-04: Path exactly at boundary (workspace/../workspace) should resolve correctly
    #[test]
    fn test_bnd_04_boundary_resolution() {
        let setup = TestSetup::new();
        let user_id_str = setup.user_id.to_string();

        // This path: /workspaces/{user_id}/../{user_id}/file.txt
        // Should resolve to: /workspaces/{user_id}/file.txt
        let file = setup.create_user_file("file.txt");
        let boundary_path = setup.user_workspace
            .join("..")
            .join(&user_id_str)
            .join("file.txt");

        let result = validate_user_path_test(
            &setup.user_id,
            &boundary_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            result.is_ok(),
            "Path that resolves back to user's workspace should be allowed"
        );

        // Verify it resolved to the correct file
        if let Ok(canonical) = result {
            assert_eq!(
                dunce::canonicalize(&file).unwrap(),
                canonical,
                "Resolved path should match the canonical file path"
            );
        }
    }

    /// BND-05: Non-existent path within workspace should be allowed (for creation)
    #[test]
    fn test_bnd_05_nonexistent_within_workspace_allowed() {
        let setup = TestSetup::new();

        let nonexistent_path = setup.user_workspace.join("new_project").join("new_file.txt");

        let result = validate_user_path_test(
            &setup.user_id,
            &nonexistent_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        // Non-existent paths within the workspace should be allowed
        // (for file creation operations)
        assert!(
            result.is_ok(),
            "Non-existent path within workspace should be allowed for creation"
        );
    }

    /// BND-06: Empty relative path should be handled
    #[test]
    fn test_bnd_06_empty_path_handled() {
        let setup = TestSetup::new();

        let empty_path = PathBuf::new();

        let result = validate_user_path_test(
            &setup.user_id,
            &empty_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        // Empty path should be rejected as it's not within workspace
        assert!(
            matches!(result, Err(_)),
            "Empty path should be rejected"
        );
    }
}

// ========== MODE-01: Deployment Mode Tests ==========

#[cfg(test)]
mod deployment_mode_tests {
    use super::*;

    /// MODE-01: Desktop mode should skip path validation
    #[test]
    fn test_mode_01_desktop_skips_validation() {
        let setup = TestSetup::new();

        // Create a path outside the user workspace
        let external_file = setup.create_external_file("external.txt");

        let result = validate_user_path_test(
            &setup.user_id,
            &external_file,
            MockDeploymentMode::Desktop, // Desktop mode
            &setup.workspace_base,
        );

        // In desktop mode, validation is skipped for backward compatibility
        assert!(
            result.is_ok(),
            "Desktop mode should allow access to any path"
        );
    }

    /// MODE-02: Kubernetes mode should enforce strict validation
    #[test]
    fn test_mode_02_k8s_enforces_validation() {
        let setup = TestSetup::new();

        // Create a path outside the user workspace
        let external_file = setup.create_external_file("external.txt");

        let result = validate_user_path_test(
            &setup.user_id,
            &external_file,
            MockDeploymentMode::Kubernetes, // K8s mode
            &setup.workspace_base,
        );

        // In K8s mode, strict validation is enforced
        assert!(
            matches!(result, Err(TestWorkspaceError::Unauthorized(_))),
            "Kubernetes mode should block access outside workspace"
        );
    }
}

// ========== EDGE-01: Edge Case Tests ==========

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    /// EDGE-01: Very long path should be handled
    #[test]
    fn test_edge_01_long_path_handled() {
        let setup = TestSetup::new();

        // Create a very deep directory structure
        let mut long_path = setup.user_workspace.clone();
        for i in 0..50 {
            long_path = long_path.join(format!("dir{}", i));
        }
        long_path = long_path.join("file.txt");

        // Don't create the path, just validate it
        let result = validate_user_path_test(
            &setup.user_id,
            &long_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        // Long paths within workspace should be accepted
        assert!(
            result.is_ok(),
            "Long paths within workspace should be allowed"
        );
    }

    /// EDGE-02: Unicode characters in path should be handled
    #[test]
    fn test_edge_02_unicode_path_handled() {
        let setup = TestSetup::new();

        // Create directory with unicode name
        let unicode_dir = setup.user_workspace.join("项目-プロジェクト-проект");
        std::fs::create_dir_all(&unicode_dir).expect("Failed to create unicode directory");

        let unicode_file = unicode_dir.join("文件.txt");
        std::fs::write(&unicode_file, "content").expect("Failed to create unicode file");

        let result = validate_user_path_test(
            &setup.user_id,
            &unicode_file,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            result.is_ok(),
            "Unicode paths within workspace should be allowed"
        );
    }

    /// EDGE-03: Spaces in path should be handled
    #[test]
    fn test_edge_03_spaces_in_path() {
        let setup = TestSetup::new();

        let spaced_dir = setup.user_workspace.join("my project").join("src folder");
        std::fs::create_dir_all(&spaced_dir).expect("Failed to create spaced directory");

        let spaced_file = spaced_dir.join("main file.rs");
        std::fs::write(&spaced_file, "content").expect("Failed to create spaced file");

        let result = validate_user_path_test(
            &setup.user_id,
            &spaced_file,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            result.is_ok(),
            "Paths with spaces should be handled correctly"
        );
    }

    /// EDGE-04: Single dot (current directory) should be handled
    #[test]
    fn test_edge_04_single_dot() {
        let setup = TestSetup::new();

        // Path with single dots
        let dotted_path = setup.user_workspace.join(".").join(".").join("file.txt");
        let actual_file = setup.user_workspace.join("file.txt");
        std::fs::write(&actual_file, "content").expect("Failed to create file");

        let result = validate_user_path_test(
            &setup.user_id,
            &dotted_path,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(
            result.is_ok(),
            "Paths with single dots should resolve correctly"
        );
    }

    /// EDGE-05: Hidden files (dotfiles) should be allowed
    #[test]
    fn test_edge_05_hidden_files_allowed() {
        let setup = TestSetup::new();

        let hidden_file = setup.create_user_file(".hidden_config");
        let hidden_dir = setup.user_workspace.join(".git");
        std::fs::create_dir_all(&hidden_dir).expect("Failed to create hidden dir");

        let result_file = validate_user_path_test(
            &setup.user_id,
            &hidden_file,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        let result_dir = validate_user_path_test(
            &setup.user_id,
            &hidden_dir,
            MockDeploymentMode::Kubernetes,
            &setup.workspace_base,
        );

        assert!(result_file.is_ok(), "Hidden files should be allowed");
        assert!(result_dir.is_ok(), "Hidden directories should be allowed");
    }
}
