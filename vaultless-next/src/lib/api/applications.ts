import { apiClient } from '../apiClient';
import type {
  CreateApplicationRequest,
  CreateApplicationResponse,
  ApplicationResponse,
  ApplicationWithUsage,
  ApplicationDashboardResponse,
  UserUsageSummary,
  QuotaWarning,
  PaginatedApplicationsSummary,
  PaginatedQuotaWarnings,
  ApplicationListParams,
  RotateSecretKeyResponse,
  RotatePublishableKeyRequest,
  RotatePublishableKeyResponse,
  AddPublishableKeyResponse,
  DeactivatePublishableKeyRequest,
} from '@/types/api';

// ============================================================================
// APPLICATIONS API
// ============================================================================

export const applicationsApi = {
  // ============================================================================
  // CRUD OPERATIONS
  // ============================================================================

  /**
   * List user's applications with pagination, search, filter, and sort
   * GET /dev/applications
   */
  list: async (
    params?: ApplicationListParams
  ): Promise<PaginatedApplicationsSummary> => {
    const queryParams: Record<string, string> = {};
    if (params) {
      if (params.page !== undefined) queryParams.page = String(params.page);
      if (params.pageSize !== undefined) queryParams.pageSize = String(params.pageSize);
      if (params.search) queryParams.search = params.search;
      if (params.sort) queryParams.sort = params.sort;
      if (params.sortOrder) queryParams.sortOrder = params.sortOrder;
      if (params.filterActive !== undefined) queryParams.filterActive = String(params.filterActive);
      if (params.filterInactive !== undefined) queryParams.filterInactive = String(params.filterInactive);
      if (params.tier) queryParams.tier = params.tier;
    }
    return apiClient.get<PaginatedApplicationsSummary>('/dev/applications', queryParams);
  },

  /**
   * Get application by ID including keys and usage
   * GET /dev/applications/{application_id}/with_keys
   */
  getWithKeys: async (
    applicationId: string
  ): Promise<ApplicationWithUsage> => {
    return apiClient.get<ApplicationWithUsage>(
      `/dev/applications/${applicationId}/with_keys`
    );
  },

  /**
   * Create a new application
   * POST /dev/applications
   */
  create: async (
    data: CreateApplicationRequest
  ): Promise<CreateApplicationResponse> => {
    return apiClient.post<CreateApplicationResponse>(
      '/dev/applications',
      data
    );
  },

  /**
   * Update application metadata
   * PATCH /api/applications/{app_id}
   */
  update: async (
    applicationId: string,
    data: Partial<{
      name: string;
      description: string;
      maxTtlSeconds: number;
      isKeyRotationForced: boolean;
      internalNotes: string;
    }>
  ): Promise<ApplicationResponse> => {
    return apiClient.patch<ApplicationResponse>(
      `/api/applications/${applicationId}`,
      data
    );
  },

  /**
   * Deactivate application
   * DELETE /api/applications/{app_id}
   */
  deactivate: async (applicationId: string): Promise<void> => {
    return apiClient.delete<void>(`/api/applications/${applicationId}`);
  },

  // ============================================================================
  // USAGE & ANALYTICS
  // ============================================================================

  /**
   * Get aggregated usage statistics across all user's applications
   * GET /dev/applications/usage-summary
   */
  getUsageSummary: async (): Promise<UserUsageSummary> => {
    return apiClient.get<UserUsageSummary>('/dev/applications/usage-summary');
  },

  /**
   * Get applications approaching or exceeding quota limits
   * GET /dev/applications/quota-warnings
   */
  getQuotaWarnings: async (params?: {
    threshold?: number;
    page?: number;
    pageSize?: number;
  }): Promise<PaginatedQuotaWarnings> => {
    return apiClient.get<PaginatedQuotaWarnings>(
      '/dev/applications/quota-warnings',
      {
        ...(params?.threshold !== undefined && {
          threshold: String(params.threshold),
        }),
        ...(params?.page && { page: String(params.page) }),
        ...(params?.pageSize && { pageSize: String(params.pageSize) }),
      }
    );
  },

  /**
   * Get full analytics for an application
   * GET /dev/applications/{application_id}/analytics
   */
  getAnalytics: async (
    applicationId: string
  ): Promise<ApplicationDashboardResponse> => {
    return apiClient.get<ApplicationDashboardResponse>(
      `/dev/applications/${applicationId}/analytics`
    );
  },

  // ============================================================================
  // KEY MANAGEMENT
  // ============================================================================

  /**
   * Rotate an application's secret key
   * POST /dev/applications/{app_id}/keys/secret/rotate
   */
  rotateSecretKey: async (
    applicationId: string
  ): Promise<RotateSecretKeyResponse> => {
    return apiClient.post<RotateSecretKeyResponse>(
      `/dev/applications/${applicationId}/keys/secret/rotate`
    );
  },

  /**
   * Rotate an application's publishable key
   * POST /dev/applications/{app_id}/keys/publishable/rotate
   */
  rotatePublishableKey: async (
    applicationId: string,
    data?: RotatePublishableKeyRequest
  ): Promise<RotatePublishableKeyResponse> => {
    return apiClient.post<RotatePublishableKeyResponse>(
      `/dev/applications/${applicationId}/keys/publishable/rotate`,
      data || {}
    );
  },

  /**
   * Add an additional publishable key
   * POST /dev/applications/{app_id}/keys/publishable
   */
  addPublishableKey: async (
    applicationId: string
  ): Promise<AddPublishableKeyResponse> => {
    return apiClient.post<AddPublishableKeyResponse>(
      `/dev/applications/${applicationId}/keys/publishable`
    );
  },

  /**
   * Deactivate a specific publishable key
   * POST /dev/applications/{app_id}/keys/publishable/deactivate
   */
  deactivatePublishableKey: async (
    applicationId: string,
    data: DeactivatePublishableKeyRequest
  ): Promise<void> => {
    return apiClient.post<void>(
      `/dev/applications/${applicationId}/keys/publishable/deactivate`,
      data
    );
  },
};

export default applicationsApi;
