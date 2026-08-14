const { withAndroidManifest } = require("expo/config-plugins");

/**
 * LAN pairing uses an encrypted ws:// transport. Android blocks cleartext
 * traffic in release builds unless the application opts in explicitly.
 */
module.exports = function withAndroidLanWebSocket(config) {
  return withAndroidManifest(config, (config) => {
    const application = config.modResults.manifest.application?.[0];
    if (application) {
      application.$ = application.$ ?? {};
      application.$["android:usesCleartextTraffic"] = "true";
    }
    return config;
  });
};
