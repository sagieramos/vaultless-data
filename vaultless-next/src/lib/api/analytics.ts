import { apiClient } from '../apiClient';
import type {
  QuotaStatusResponse,
  CostBreakdownResponse,
  TrendsResponse,
  ExportQuery,
  ExportFormat,
} from '@/types/api';

// ============================================================================
// ANALYTICS API
// ============================================================================

export const analyticsApi = {
  /**
   * Get real-time quota status for a specific application
   * GET /dev/applications/{application_id}/quota-status
   */
  getQuotaStatus: async (
    applicationId: string
  ): Promise<QuotaStatusResponse> => {
    return apiClient.get<QuotaStatusResponse>(
      `/dev/applications/${applicationId}/quota-status`
    );
  },

  /**
   * Get detailed cost breakdown for an application
   * GET /dev/applications/{application_id}/costs
   */
  getCostBreakdown: async (
    applicationId: string
  ): Promise<CostBreakdownResponse> => {
    return apiClient.get<CostBreakdownResponse>(
      `/dev/applications/${applicationId}/costs`
    );
  },

  /**
   * Export application usage data in JSON or CSV format
   * GET /dev/applications/{application_id}/export
   */
  exportUsage: async (
    applicationId: string,
    format: ExportFormat = 'json'
  ): Promise<any> => {
    return apiClient.get(`/dev/applications/${applicationId}/export`, {
      format,
    });
  },

  /**
   * Get usage trends for an application
   * GET /dev/applications/{application_id}/trends
   */
  getTrends: async (applicationId: string): Promise<TrendsResponse> => {
    return apiClient.get<TrendsResponse>(
      `/dev/applications/${applicationId}/trends`
    );
  },

  /**
   * Export application usage as CSV
   * This downloads a file with appropriate headers
   */
  exportUsageAsCsv: async (
    applicationId: string,
    applicationName: string
  ): Promise<void> => {
    const response = await fetch(
      `${process.env.NEXT_PUBLIC_API_URL}/dev/applications/${applicationId}/export?format=csv`,
      {
        method: 'GET',
        headers: {
          Authorization: `Bearer ${localStorage.getItem('access_token')}`,
        },
      }
    );

    if (!response.ok) {
      throw new Error('Failed to export CSV');
    }

    const blob = await response.blob();
    const url = window.URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${applicationName.replace(/ /g, '_')}_usage.csv`;
    document.body.appendChild(a);
    a.click();
    window.URL.revokeObjectURL(url);
    document.body.removeChild(a);
  },
};

export default analyticsApi;
