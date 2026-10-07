# muniment desktop reviewer notes

- Hold the diff against `SPEC.md` and, for any visual or copy change, `DESIGN.md`.
- The local product needs no account and sends no data anywhere. Block a change that adds a network call the user did not choose.
- Block a credential in the tree, a change to signing, notarization or release promotion, and a change to the app version.
- Read every `#[cfg(windows)]` and `#[cfg(target_os = "macos")]` change line by line. CI is its only compiler.
- For a CI-fix bundle, check that every listed failure has a fix and a named proving check. Block a bundle that drops an item without saying why.
- Check that an edited assertion did not get weaker and that new code has tests.
