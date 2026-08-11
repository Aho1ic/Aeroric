const { withEntitlementsPlist } = require("expo/config-plugins");

/**
 * Aeroric only schedules local notifications. expo-notifications adds the
 * APNs entitlement during prebuild, but free Personal Team provisioning does
 * not need it and may reject it. Keep the native notifications module while
 * removing only the remote-push entitlement.
 */
module.exports = function withLocalNotificationsOnly(config) {
  return withEntitlementsPlist(config, (config) => {
    delete config.modResults["aps-environment"];
    return config;
  });
};
