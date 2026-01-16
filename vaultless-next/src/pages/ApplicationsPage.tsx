"use client";
import { useState, useEffect } from 'react';
import Link from 'next/link';
import { motion } from 'motion/react';
import { Plus, BarChart3, Settings, Server, MessageSquare, Loader2, Key, Cpu, Wifi } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Progress } from '../components/ui/progress';
import DashboardLayout from '../components/layout/DashboardLayout';
import { useRequireAuth } from '@/contexts/AuthContext';
import { applicationsApi } from '@/lib/api';
import { formatRelativeTime } from '@/lib/date';
import type { ApplicationListParams, PaginatedApplicationsSummary } from '@/types/api';
import { ApplicationsFilter, type FilterState } from '../components/applications/ApplicationsFilter';
import {
  Pagination,
  PaginationContent,
  PaginationEllipsis,
  PaginationItem,
  PaginationLink,
  PaginationNext,
  PaginationPrevious,
} from '../components/ui/pagination';

export default function ApplicationsPage() {
  const { isAuthenticated, isLoading } = useRequireAuth();

  // Use the generated PaginatedApplicationsSummary type from the API types and extend it with
  // a few optional UI-only fields we sometimes display (keys, webhooks, quota shapes, etc.).
  type AppItem = PaginatedApplicationsSummary['data'][number] & Partial<{
    quota: { used?: number; limit?: number } | null;
    keys: number;
    webhooks: number;
    type: string;
    is_active: boolean;
    active: boolean;
    status: string;
    created_at: string;
  }>;

  const [applications, setApplications] = useState<AppItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filters, setFilters] = useState<FilterState>({
    sort: 'createdAt',
    sortOrder: 'desc',
  });
  const [pagination, setPagination] = useState({
    page: 1,
    pageSize: 9,
    totalCount: 0,
    totalPages: 1,
  });

  useEffect(() => {
    // Don't fetch until auth is resolved and user is authenticated
    if (isLoading) return;
    if (!isAuthenticated) return;

    fetchApplications();
  }, [filters, pagination.page, pagination.pageSize, isAuthenticated, isLoading]);

  const fetchApplications = async () => {
    setLoading(true);
    try {
      const params: ApplicationListParams = {
        page: pagination.page,
        pageSize: pagination.pageSize,
        ...filters,
      };

      const res = await applicationsApi.list(params);
      setApplications(res.data || []);
      setPagination(prev => ({
        ...prev,
        totalCount: res.totalCount,
        totalPages: res.totalPages,
      }));
      setError(null);
    } catch (err: any) {
      console.error('Failed to fetch applications:', err);
      setError(err.message || 'Failed to load applications');
    } finally {
      setLoading(false);
    }
  };

  const handleFilterChange = (newFilters: FilterState) => {
    setFilters(newFilters);
    setPagination(prev => ({ ...prev, page: 1 })); // Reset to page 1 on filter change
  };

  const handlePageChange = (newPage: number) => {
    setPagination(prev => ({ ...prev, page: newPage }));
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const renderPagination = () => {
    const { page, totalPages } = pagination;
    if (totalPages <= 1) return null;

    const pages = [];
    const showEllipsis = totalPages > 7;

    if (!showEllipsis) {
      for (let i = 1; i <= totalPages; i++) {
        pages.push(i);
      }
    } else {
      if (page <= 3) {
        pages.push(1, 2, 3, 4, 'ellipsis', totalPages);
      } else if (page >= totalPages - 2) {
        pages.push(1, 'ellipsis', totalPages - 3, totalPages - 2, totalPages - 1, totalPages);
      } else {
        pages.push(1, 'ellipsis', page - 1, page, page + 1, 'ellipsis', totalPages);
      }
    }

    return (
      <Pagination>
        <PaginationContent>
          <PaginationItem>
            <PaginationPrevious
              onClick={() => handlePageChange(page - 1)}
              className={page === 1 ? 'pointer-events-none opacity-50' : 'cursor-pointer'}
            />
          </PaginationItem>
          {pages.map((p, i) =>
            p === 'ellipsis' ? (
              <PaginationItem key={`ellipsis-${i}`}>
                <PaginationEllipsis />
              </PaginationItem>
            ) : (
              <PaginationItem key={`page-${p}`}>
                <PaginationLink
                  onClick={() => handlePageChange(p as number)}
                  isActive={page === p}
                  className="cursor-pointer"
                >
                  {p}
                </PaginationLink>
              </PaginationItem>
            )
          )}
          <PaginationItem>
            <PaginationNext
              onClick={() => handlePageChange(page + 1)}
              className={page === totalPages ? 'pointer-events-none opacity-50' : 'cursor-pointer'}
            />
          </PaginationItem>
        </PaginationContent>
      </Pagination>
    );
  };

  return (
    <DashboardLayout>
      <div className="mb-8 flex flex-col md:flex-row md:items-center md:justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white">Applications</h1>
          <p className="text-gray-600 dark:text-gray-400 mt-1">
            Manage your applications and API integrations
          </p>
        </div>
        <Link href="/applications/new">
          <Button className="bg-blue-600 hover:bg-blue-700">
            <Plus className="w-4 h-4 mr-2" />
            Create App
          </Button>
        </Link>
      </div>

      {/* Filters */}
      <ApplicationsFilter
        filters={filters}
        onFilterChange={handleFilterChange}
        totalCount={pagination.totalCount}
      />

      {/* Loading State */}
      {loading && (
        <div className="flex items-center justify-center py-12">
          <Loader2 className="w-8 h-8 animate-spin text-blue-600" />
          <span className="ml-3 text-gray-600 dark:text-gray-400">Loading applications...</span>
        </div>
      )}

      {/* Error State */}
      {error && (
        <Card className="p-6 border-red-200 bg-red-50 dark:bg-red-900/10">
          <p className="text-red-600 dark:text-red-400">{error}</p>
        </Card>
      )}

      {/* Empty State */}
      {!loading && !error && applications.length === 0 && (
        <Card className="p-12 text-center">
          <Server className="w-16 h-16 mx-auto text-gray-400 mb-4" />
          <h3 className="text-xl font-semibold text-gray-900 dark:text-white mb-2">
            {filters.search || filters.filterActive !== undefined || filters.filterInactive !== undefined || filters.tier
              ? 'No Matching Applications'
              : 'No Applications Yet'}
          </h3>
          <p className="text-gray-600 dark:text-gray-400 mb-6">
            {filters.search || filters.filterActive !== undefined || filters.filterInactive !== undefined || filters.tier
              ? 'Try adjusting your filters or search terms'
              : 'Create your first application to start using Vaultless'}
          </p>
          <Link href="/applications/new">
            <Button>
              <Plus className="w-4 h-4 mr-2" />
              Create Application
            </Button>
          </Link>
        </Card>
      )}

      {/* Applications Grid */}
      {!loading && !error && applications.length > 0 && (
        <>
          <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-6">
            {applications.map((app, index) => {
              console.log(app);
              const isActive = (app as any).isActive ?? (app as any).is_active ?? (app as any).active ?? ((app as any).status === 'active');
              const quotaLimit = (app as any).quota?.limit ?? null;
              const quotaUsed = (app as any).quota?.used ?? null;
              const monthlyQuota = (app as any).monthlyMessageQuota ?? null;
              const currentMonthMessages = (app as any).currentMonthMessagesSent ?? null;
              const rawQuotaUsage = (app as any).quotaUsagePercentage;

              const tierRaw = (app as any).tier ?? 'Free';
              const tier = String(tierRaw);
              const isPremiumTier = ['pro', 'enterprise'].includes(tier.toLowerCase());
              const tierLabel = tier.charAt(0).toUpperCase() + tier.slice(1);

              let quotaUsagePercentageNumber = 0;
              if (quotaLimit !== null && quotaUsed !== null && quotaUsed !== undefined) {
                quotaUsagePercentageNumber = (quotaUsed / quotaLimit) * 100;
              } else if (typeof rawQuotaUsage === 'number') {
                quotaUsagePercentageNumber = rawQuotaUsage * 100;
              } else if (typeof rawQuotaUsage === 'string') {
                quotaUsagePercentageNumber = parseFloat(rawQuotaUsage.replace('%','')) || 0;
              } else if (monthlyQuota !== null && currentMonthMessages !== null && currentMonthMessages !== undefined) {
                quotaUsagePercentageNumber = (currentMonthMessages / monthlyQuota) * 100;
              }
              quotaUsagePercentageNumber = Math.min(100, Math.max(0, Math.round(quotaUsagePercentageNumber)));
              const quotaPercentageDisplay = quotaLimit ? `${Math.round((quotaUsed! / quotaLimit) * 100)}%` : `${quotaUsagePercentageNumber}%`;

              return (
              <motion.div
                key={app.id}
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ delay: index * 0.1 }}
              >
                <Card className={`p-6 hover:shadow-lg transition-shadow ${app.type === 'iot' ? 'border-purple-200 dark:border-purple-800 bg-purple-50/50 dark:bg-purple-900/10' : ''}`}>
                  <div className="flex items-start justify-between mb-4">
                    <div className="flex-1">
                      <div className="flex items-center gap-2 mb-1">
                        {app.type === 'iot' ? (
                          <div className="w-8 h-8 bg-purple-100 dark:bg-purple-900/30 rounded-lg flex items-center justify-center">
                            <Cpu className="w-4 h-4 text-purple-600" />
                          </div>
                        ) : (
                          <div className="w-8 h-8 bg-blue-100 dark:bg-blue-900/30 rounded-lg flex items-center justify-center">
                            <MessageSquare className="w-4 h-4 text-blue-600" />
                          </div>
                        )}
                        <h3 className="font-semibold text-lg text-gray-900 dark:text-white truncate">
                          {app.name}
                        </h3>
                        {isActive && (
                          <div className="w-2 h-2 rounded-full bg-green-500 flex-shrink-0 ml-1" />
                        )} 
                      </div>
                      <p className="text-sm text-gray-600 dark:text-gray-400 line-clamp-1">
                        {app.description}
                      </p>
                    </div>
                    <div className="flex flex-col items-end gap-1">
                      <Badge variant={isPremiumTier ? 'default' : 'secondary'}>
                        {tierLabel}
                      </Badge>
                      {app.type === 'iot' && (
                        <Badge variant="outline" className="text-xs text-purple-600 border-purple-300">
                          <Wifi className="w-3 h-3 mr-1" />
                          IoT
                        </Badge>
                      )}
                    </div>
                  </div>

                  {/* Quota (for all apps) */}
                  {(quotaLimit !== null || rawQuotaUsage !== undefined || monthlyQuota !== null) && (
                    <div className="mb-4">
                      <div className="flex items-center justify-between mb-2">
                        <span className="text-sm text-gray-600 dark:text-gray-400">Quota</span>
                        <span className="text-sm font-medium text-gray-900 dark:text-white">
                          {quotaPercentageDisplay}
                        </span>
                      </div>

                      {quotaLimit !== null ? (
                        <Progress
                          value={(quotaUsed! / quotaLimit) * 100}
                          className="h-2"
                        />
                      ) : (
                        <div className="h-1.5 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                          <div
                            className={`h-full rounded-full transition-all bg-green-500`}
                            style={{ width: `${quotaUsagePercentageNumber}%` }}
                          />
                        </div>
                      )}

                      <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                        {quotaLimit !== null
                          ? `${quotaUsed?.toLocaleString() ?? 0} / ${quotaLimit >= 999999 ? 'Unlimited' : quotaLimit.toLocaleString()} messages`
                          : monthlyQuota !== null
                            ? `${currentMonthMessages ?? 0} / ${monthlyQuota} messages`
                            : `${quotaPercentageDisplay} used`}
                      </p>
                    </div>
                  )} 

                  {/* Stats */}
                  <div className="flex items-center gap-4 mb-4 text-sm text-gray-600 dark:text-gray-400">
                    <span className="flex items-center gap-1">
                      <Key className="w-4 h-4" /> {app.keys || 0} keys
                    </span>
                    <span className="flex items-center gap-1">
                      <Server className="w-4 h-4" /> {app.webhooks || 0} webhooks
                    </span>
                  </div>

                  <p className="text-xs text-gray-600 dark:text-gray-400 mb-4">
                    Created {(app.createdAt || app.created_at) ? formatRelativeTime(app.createdAt ?? app.created_at) : 'Unknown'}
                  </p>

                  {/* Actions */}
                  <div className="flex gap-2">
                    <Link href={`/applications/${app.id}`} className="flex-1">
                      <Button variant="outline" size="sm" className="w-full">
                        <Settings className="w-4 h-4 mr-2" />
                        {app.type === 'iot' ? 'Devices' : 'Manage'}
                      </Button>
                    </Link>
                    <Link href={`/applications/${app.id}/analytics`}>
                      <Button variant="outline" size="sm">
                        <BarChart3 className="w-4 h-4" />
                      </Button>
                    </Link>
                    </div>
                  </Card>
                </motion.div>
            );
            })}
          </div>

          {renderPagination()}
        </>
      )}
    </DashboardLayout>
  );
}
