cask "snapt" do
  arch arm: "aarch64", intel: "x64"

  version "2.1.0"
  sha256 arm:   "59f71d33fa31df873bea29532435271e1a3c9cefa5c2cb3f5b3b040c10bfc3d5",
         intel: "953c16fd92cfa957c2f4d4d6a0141b8c36a07712da8d7ec2f5e0b49283aaa2af"

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
