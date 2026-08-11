const { withXcodeProject } = require("expo/config-plugins");
const path = require("path");
const fs = require("fs");

/**
 * Sets the Aeroric scheme's LaunchAction buildConfiguration to Release so that
 * running on a physical device always installs a self-contained Release build
 * (Hermes bundle embedded, no Metro dependency).
 */
module.exports = function withReleaseLaunchScheme(config) {
  return withXcodeProject(config, (config) => {
    const projectRoot = config.modRequest.projectRoot;
    const schemePath = path.join(
      projectRoot,
      "ios",
      "Aeroric.xcodeproj",
      "xcshareddata",
      "xcschemes",
      "Aeroric.xcscheme",
    );

    if (!fs.existsSync(schemePath)) {
      console.warn("[with-release-launch-scheme] scheme file not found:", schemePath);
      return config;
    }

    let contents = fs.readFileSync(schemePath, "utf8");

    // Replace only the LaunchAction buildConfiguration (not TestAction / AnalyzeAction)
    contents = contents.replace(
      /(<LaunchAction[^>]*\s)buildConfiguration\s*=\s*"Debug"/,
      '$1buildConfiguration = "Release"',
    );

    fs.writeFileSync(schemePath, contents, "utf8");
    return config;
  });
};
