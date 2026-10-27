cask "snapt" do
  arch arm: "aarch64", intel: "x64"

  version "2.2.0"
  sha256 arm:   "4d905e17feb3b48dc5fe42211de92c0c2bce62986f8b58ff7f23d82a6986c1b7",
         intel: "d176498d63dd90f34d1cffca8f24fc4ccdafefc455a70e18c0a6c8e06e2362db"

  url "https://github.com/vkardin/snapt/releases/download/v#{version}/Snapt_#{version}_#{arch}.dmg"
  name "Snapt"
  desc "Fast, lightweight, open-source screenshot tool"
  homepage "https://vkardin.github.io/snapt/"

  livecheck do
    url :url
    strategy :github_latest
  end

  # Snapt updates itself from GitHub Releases.
  auto_updates true
  depends_on macos: ">= :big_sur"

  app "Snapt.app"

  uninstall launchctl: "io.github.vkardin.snapt",
            quit:      "io.github.vkardin.snapt"

  zap trash: [
    "~/Library/Application Support/io.github.vkardin.Snapt",
    "~/Library/LaunchAgents/io.github.vkardin.snapt.plist",
  ]

  caveats <<~EOS
    Snapt is not notarized yet. If macOS refuses to open it, run:
      xattr -dr com.apple.quarantine #{appdir}/Snapt.app
  EOS
end
