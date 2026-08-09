cask "muniment-nightly" do
  version "4d27dd80197ff2eb45f0c1dc999514c085852327"
  sha256 "bd3249a977f48da50b0a661f91b4c9843adb22279d1eb4950bce64e7c807201a"

  url "https://github.com/mikeydiamonds/muniment-desktop/releases/download/nightly/nightly-#{version}-macos-muniment.app.zip"
  name "Muniment Nightly"
  desc "Desktop client for Muniment"
  homepage "https://github.com/mikeydiamonds/muniment-desktop"

  app "muniment.app"

  caveats <<~EOS
    This nightly app is unsigned until Apple completes the developer enrollment.
    macOS Gatekeeper will block the app after installation.

    If you knowingly trust this download, remove its quarantine attribute:
      xattr -dr com.apple.quarantine /Applications/muniment.app
  EOS
end
