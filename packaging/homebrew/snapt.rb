cask "snapt" do
  arch arm: "aarch64", intel: "x64"

  version "1.3.0"
  sha256 arm:   "2b360aa8170aeab788569afeec3c0b3532dce9bda0677e43c7d09ff6db0f579e",
         intel: "cae02925cf71f814a6ba35fa529c180281a826c35b2cb690a7f92d83bf82ed57"

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
