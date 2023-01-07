use std::fs::File;

use camino::Utf8PathBuf;
use miette::{IntoDiagnostic, WrapErr};
use tracing::warn;

const GITHUB_CI_PART1: &str = r###"
# CI that:
#
# * checks for a Git Tag that looks like a release ("v1.2.0")
# * creates a Github Release™️
# * builds binaries/packages with cargo-dist
# * uploads those packages to the Github Release™️
#
# Note that the Github Release™️ will be created before the packages,
# so there will be a few minutes where the release has no packages
# and then they will slowly trickle in, possibly failing. To make
# this more pleasant we mark the release as a "draft" until all
# artifacts have been successfully uploaded. This allows you to
# choose what to do with partial successes and avoids spamming
# anyone with notifications before the release is actually ready.
name: Release

permissions:
  contents: write

# This task will run whenever you push a git tag that looks like
# a version number. We just look for `v` followed by at least one number
# and then whatever. so `v1`, `v1.0.0`, and `v1.0.0-prerelease` all work.
#
# If there's a prerelease-style suffix to the version then the Github Release™️
# will be marked as a prerelease (handled by taiki-e/create-gh-release-action).
on:
  push:
    tags:
      - v[0-9]+.*

env:"###;

const GITHUB_CI_PART2: &str = r###"
jobs:
  # Create the Github Release™️ so the packages have something to be uploaded to
  create-release:
    runs-on: ubuntu-latest
    outputs:
      tag: ${{ steps.create-gh-release.outputs.computed-prefix }}${{ steps.create-gh-release.outputs.version }}
    steps:
      - uses: actions/checkout@v3
      - id: create-gh-release
        uses: taiki-e/create-gh-release-action@v1
        with:
          # (optional) Path to changelog. This will used to for the body of the Github Releaase™️
          # changelog: RELEASES.md
          draft: true
          # (required) GitHub token for creating GitHub Releases.
          token: ${{ secrets.GITHUB_TOKEN }}


  # Build and packages all the things
  upload-artifacts:
    needs: create-release
    strategy:
      matrix:
        # For these target platforms
        include:"###;

const GITHUB_CI_PART3: &str = r###"
    runs-on: ${{ matrix.os }}
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    steps:
      - uses: actions/checkout@v3
      - name: Install Rust
        run: rustup update stable && rustup default stable
      - name: Install cargo-dist
        # Currently we install cargo-dist from git, in the future when it's
        # published on crates.io or has prebuilt binaries, we'll do better.
        run: cargo install --git https://github.com/axodotdev/cargo-dist/
      - name: Run cargo-dist
        # This logic is a bit janky because it's trying to be a polyglot between
        # powershell and bash since this will run on windows, macos, and linux!
        # The two platforms don't agree on how to talk about env vars but they
        # do agree on 'cat' and '$()' so we use that to marshal values between commmands.
        run: |
          cargo dist --output-format=json > dist-manifest.json
          echo "dist ran successfully"
          cat dist-manifest.json
          cat dist-manifest.json | jq --raw-output ".releases[].artifacts[].path" > uploads.txt
          echo "uploading..."
          cat uploads.txt
          gh release upload ${{ needs.create-release.outputs.tag }} $(cat uploads.txt)
          echo "uploaded!"

  # Compute and upload the manifest for everything
  upload-manifest:
    needs: create-release
    runs-on: ubuntu-latest
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    steps:
      - uses: actions/checkout@v3
      - name: Install Rust
        run: rustup update stable && rustup default stable
      - name: Install cargo-dist
        # Currently we install cargo-dist from git, in the future when it's
        # published on crates.io or has prebuilt binaries, we'll do better.
        run: cargo install --git https://github.com/axodotdev/cargo-dist/
      - name: Run cargo-dist
        run: |
          cargo dist manifest --output-format=json $ALL_CARGO_DIST_TARGET_ARGS > dist-manifest.json
          echo "dist ran successfully"
          cat dist-manifest.json
          gh release upload ${{ needs.create-release.outputs.tag }} dist-manifest.json
          echo "uploaded!"


  # Mark the Github Release™️ as a non-draft now that everything has succeeded!
  publish-release:
    needs: [create-release, upload-artifacts, upload-manifest]
    runs-on: ubuntu-latest
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    steps:
      - uses: actions/checkout@v3
      - name: mark release as non-draft
        run: |
          gh release edit ${{ needs.create-release.outputs.tag }} --draft=false
"###;

pub fn generate_github_ci(
    workspace_dir: &Utf8PathBuf,
    targets: &[String],
) -> Result<(), miette::Report> {
    const GITHUB_CI_DIR: &str = ".github/workflows/";
    const GITHUB_CI_FILE: &str = "release.yml";

    // FIXME: should we try to avoid clobbering old files..?
    let ci_dir = workspace_dir.join(GITHUB_CI_DIR);
    let ci_file = ci_dir.join(GITHUB_CI_FILE);
    std::fs::create_dir_all(&ci_dir)
        .into_diagnostic()
        .wrap_err("Failed to create ci dir")?;
    let mut file = File::create(ci_file)
        .into_diagnostic()
        .wrap_err("Failed to create ci file")?;
    write_github_ci(&mut file, targets)
        .into_diagnostic()
        .wrap_err("Failed to write to CI file")?;
    Ok(())
}

fn write_github_ci(f: &mut File, targets: &[String]) -> Result<(), std::io::Error> {
    use std::io::Write;

    writeln!(f, "{GITHUB_CI_PART1}")?;

    // Write out target args
    let mut target_args = Vec::new();
    for target in targets {
        write!(&mut target_args, "--target={target} ")?;
    }
    writeln!(
        f,
        "  ALL_CARGO_DIST_TARGET_ARGS: {}",
        String::from_utf8(target_args).unwrap()
    )?;

    writeln!(f, "{GITHUB_CI_PART2}")?;

    for target in targets {
        let Some(os) = github_os_for_target(target) else {
            warn!("skipping generating ci for {target} (no idea what github os should build this)");
            continue;
        };
        writeln!(f, "        - target: {target}")?;
        writeln!(f, "          os: {os}")?;
    }

    writeln!(f, "{GITHUB_CI_PART3}")?;

    Ok(())
}

fn github_os_for_target(target: &str) -> Option<&'static str> {
    if target.contains("linux") {
        Some("ubuntu-latest")
    } else if target.contains("apple") {
        Some("macos-latest")
    } else if target.contains("windows") {
        Some("windows-latest")
    } else {
        None
    }
}