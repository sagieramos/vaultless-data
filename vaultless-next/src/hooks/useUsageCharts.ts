import { useQuery } from '@tanstack/react-query';
import { analyticsApi } from '@/lib/api/analytics';
import type { UsageOverTime } from '@/types/api';

export const useUsageCharts = (
  period: '7d' | '30d' | '90d' = '30d'
): { data: UsageOverTime | undefined; isLoading: boolean; error: unknown } => {
  return useQuery({
    queryKey: ['usage-charts', period],
    queryFn: () => analyticsApi.getUsageOverTime(period),
  });
};