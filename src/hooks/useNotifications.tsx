import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { NotificationItem, NotificationResult } from "../types";
import { useI18n } from "../i18n";

interface NotificationsContextValue {
  result: NotificationResult | null;
  loading: boolean;
  error: string | null;
  latestUpdate: NotificationItem | null;
  fetchNotifications: (force?: boolean) => Promise<void>;
  markRead: (id: string) => Promise<void>;
  markAllRead: () => Promise<void>;
}

const NotificationsContext = createContext<NotificationsContextValue | null>(null);

export function NotificationsProvider({ children }: { children: React.ReactNode }) {
  const { t } = useI18n();
  const [result, setResult] = useState<NotificationResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchNotifications = useCallback(
    async (force = true) => {
      setLoading(true);
      try {
        const data = await invoke<NotificationResult>("get_notifications", { force });
        setResult(data);
        setError(null);
      } catch (err) {
        const message =
          typeof err === "string"
            ? err
            : err instanceof Error
              ? err.message
              : t("notification.loadingFailed");
        setError(message);
        console.error("Failed to load notifications:", err);
      } finally {
        setLoading(false);
      }
    },
    [t],
  );

  useEffect(() => {
    fetchNotifications(true);
    const POLL_INTERVAL_MS = 60 * 1000;
    const interval = setInterval(() => fetchNotifications(false), POLL_INTERVAL_MS);
    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") void fetchNotifications(false);
    };
    const handleFocus = () => {
      void fetchNotifications(false);
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("focus", handleFocus);
    return () => {
      clearInterval(interval);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("focus", handleFocus);
    };
  }, [fetchNotifications]);

  const markRead = useCallback(async (id: string) => {
    try {
      await invoke("mark_notification_read", { id });
      setResult((prev) => {
        if (!prev) return prev;
        const notifications = prev.notifications.map((n) =>
          n.id === id ? { ...n, isRead: true } : n,
        );
        const unreadCount = notifications.filter((n) => !n.isRead).length;
        return { notifications, unreadCount };
      });
    } catch (err) {
      console.error("Failed to mark notification as read:", err);
    }
  }, []);

  const markAllRead = useCallback(async () => {
    try {
      await invoke("mark_all_notifications_read");
      setResult((prev) => {
        if (!prev) return prev;
        const notifications = prev.notifications.map((n) => ({ ...n, isRead: true }));
        return { notifications, unreadCount: 0 };
      });
    } catch (err) {
      console.error("Failed to mark all notifications as read:", err);
    }
  }, []);

  const latestUpdate = useMemo(() => {
    if (!result) return null;
    return result.notifications.find((n) => n.newerThanCurrent && n.releaseTag) ?? null;
  }, [result]);

  const value = useMemo(
    () => ({
      result,
      loading,
      error,
      latestUpdate,
      fetchNotifications,
      markRead,
      markAllRead,
    }),
    [error, fetchNotifications, latestUpdate, loading, markAllRead, markRead, result],
  );

  return <NotificationsContext.Provider value={value}>{children}</NotificationsContext.Provider>;
}

export function useNotifications() {
  const context = useContext(NotificationsContext);
  if (!context) {
    throw new Error("useNotifications must be used within NotificationsProvider");
  }
  return context;
}
