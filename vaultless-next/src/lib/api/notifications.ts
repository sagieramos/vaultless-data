import { apiClient } from '../apiClient';
import type {
  Notification,
  NotificationQuery,
  PaginatedNotifications,
  UnreadCountResponse,
  NotificationSummary,
  MarkAllReadResponse,
  DeleteResponse,
  DeleteAllReadResponse,
} from '@/types/api';

// ============================================================================
// NOTIFICATIONS API
// ============================================================================

export const notificationsApi = {
  /**
   * List notifications for the current user
   * GET /dev/notifications
   */
  list: async (
    query?: NotificationQuery
  ): Promise<PaginatedNotifications> => {
    return apiClient.get<PaginatedNotifications>('/dev/notifications', {
      ...(query?.isRead !== undefined && { is_read: String(query.isRead) }),
      ...(query?.notificationType && {
        notification_type: query.notificationType,
      }),
      ...(query?.severity && { severity: query.severity }),
      ...(query?.page && { page: String(query.page) }),
      ...(query?.pageSize && { pageSize: String(query.pageSize) }),
    });
  },

  /**
   * Get a specific notification by ID
   * GET /dev/notifications/{notification_id}
   */
  get: async (notificationId: string): Promise<Notification> => {
    return apiClient.get<Notification>(
      `/dev/notifications/${notificationId}`
    );
  },

  /**
   * Get unread notification count
   * GET /dev/notifications/unread-count
   */
  getUnreadCount: async (): Promise<UnreadCountResponse> => {
    return apiClient.get<UnreadCountResponse>('/dev/notifications/unread-count');
  },

  /**
   * Get notification summary grouped by type and severity
   * GET /dev/notifications/summary
   */
  getSummary: async (): Promise<NotificationSummary[]> => {
    return apiClient.get<NotificationSummary[]>('/dev/notifications/summary');
  },

  /**
   * Mark a notification as read
   * POST /dev/notifications/{notification_id}/read
   */
  markAsRead: async (notificationId: string): Promise<Notification> => {
    return apiClient.post<Notification>(
      `/dev/notifications/${notificationId}/read`
    );
  },

  /**
   * Mark all notifications as read
   * POST /dev/notifications/read-all
   */
  markAllAsRead: async (): Promise<MarkAllReadResponse> => {
    return apiClient.post<MarkAllReadResponse>('/dev/notifications/read-all');
  },

  /**
   * Delete a notification
   * DELETE /dev/notifications/{notification_id}
   */
  delete: async (notificationId: string): Promise<DeleteResponse> => {
    return apiClient.delete<DeleteResponse>(
      `/dev/notifications/${notificationId}`
    );
  },

  /**
   * Delete all read notifications
   * DELETE /dev/notifications/read
   */
  deleteAllRead: async (): Promise<DeleteAllReadResponse> => {
    return apiClient.delete<DeleteAllReadResponse>('/dev/notifications/read');
  },
};

export default notificationsApi;
