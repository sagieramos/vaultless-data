"use client";

export const dynamic = 'force-dynamic';

import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import Link from 'next/link';
import { motion } from 'motion/react';
import { Plus, BarChart3, Settings, Server, Loader2, Key, MessageSquare } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Progress } from '@/components/ui/progress';
import DashboardLayout from '@/components/layout/DashboardLayout';
import { useRequireAuth } from '@/contexts/AuthContext';
import { applicationsApi } from '@/lib/api';
import { formatRelativeTime } from '@/lib/date';
import type { ApplicationListParams, PaginatedApplicationsSummary, ApplicationSummary } from '@/types/api';
import { ApplicationsFilter, type FilterState } from '@/components/applications/ApplicationsFilter';
import {
  Pagination,
  PaginationContent,
  PaginationEllipsis,
  PaginationItem,
  PaginationLink,
  PaginationNext,
  PaginationPrevious,
} from '@/components/ui/pagination';

export default function ApplicationsPage() {
  const { isAuthenticated, isLoading: authLoading } = useRequireAuth();
  const queryClient = useQueryClient();

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

  const { data, isLoading: applicationsLoading, error: applicationsError } = useQuery({
    queryKey: ['applications', filters, pagination.page, pagination.pageSize],
    queryFn: async () => {
      const params: ApplicationListParams = {
        page: pagination.page,
        pageSize: pagination.pageSize,
        ...filters,
      };
      return applicationsApi.list(params);
    },
    enabled: isAuthenticated && !authLoading,
  });

  const applications = data?.data || [];
  const totalCount = data?.totalCount || 0;
  const totalPages = data?.totalPages || 1;

  const handleFilterChange = (newFilters: FilterState) => {
    setFilters(newFilters);
    setPagination(prev => ({ ...prev, page: 1 }));
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

  if (authLoading) {
    return (
      <DashboardLayout>
        <div className="flex items-center justify-center min-h-screen">
          <div>Loading...</div>
        </div>
      </DashboardLayout>
    );
  }

  if (!isAuthenticated) {
    return null;
  }

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

      <ApplicationsFilter
        filters={filters}
        onFilterChange={handleFilterChange}
        totalCount={totalCount}
      />

      {applicationsLoading && (
        <div className="flex items-center justify-center py-12">
          <Loader2 className="w-8 h-8 animate-spin text-blue-600" />
          <span className="ml-3 text-gray-600 dark:text-gray-400">Loading applications...</span>
        </div>
      )}

      {applicationsError && (
        <Card className="p-6 border-red-200 bg-red-50 dark:bg-red-900/10">
          <p className="text-red-600 dark:text-red-400">
            {applicationsError instanceof Error ? applicationsError.message : 'Failed to load applications'}
          </p>
        </Card>
      )}

      {!applicationsLoading && !applicationsError && applications.length === 0 && (
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

      {!applicationsLoading && !applicationsError && applications.length > 0 && (
        <>
          <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-6">
            {applications.map((app: ApplicationSummary, index) => {
              const isActive = app.isActive ?? false;
                            const tierRaw = app.tier ?? 'Free';
              const tier = String(tierRaw).toLowerCase();
              const isPremiumTier = ['pro', 'enterprise'].includes(tier);
              const tierLabel = tierRaw?.charAt(0).toUpperCase() + tierRaw?.slice(1) || 'Free';

              let quotaUsagePercentageNumber = Math.round(app.quotaUsagePercentage * 100);
              quotaUsagePercentageNumber = Math.min(100, Math.max(0, quotaUsagePercentageNumber));

              return (
                <motion.div
                  key={app.id}
                  initial={{ opacity: 0, y: 20 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ delay: index * 0.1 }}
                >
                  <Card className="p-6 hover:shadow-lg transition-shadow">
                    <div className="flex items-start justify-between mb-4">
                      <div className="flex-1">
                        <div className="flex items-center gap-2 mb-1">
                          <div className="w-8 h-8 bg-blue-100 dark:bg-blue-900/30 rounded-lg flex items-center justify-center">
                            <MessageSquare className="w-4 h-4 text-blue-600" />
                          </div>
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
                      </div>
                    </div>

                    <div className="mb-4">
                      <div className="flex items-center justify-between mb-2">
                        <span className="text-sm text-gray-600 dark:text-gray-400">Quota</span>
                        <span className="text-sm font-medium text-gray-900 dark:text-white">
                          {quotaUsagePercentageNumber}%
                        </span>
                      </div>
                      <div className="h-1.5 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                        <div
                          className="h-full rounded-full transition-all bg-green-500"
                          style={{ width: `${quotaUsagePercentageNumber}%` }}
                        />
                      </div>
                    </div>

                    <div className="flex items-center gap-4 mb-4 text-sm text-gray-600 dark:text-gray-400">
                      <span className="flex items-center gap-1">
                        <Key className="w-4 h-4" /> {app.publishableKeyCount || 0} keys
                      </span>
                      <span className="flex items-center gap-1">
                        <Server className="w-4 h-4" /> {app.webhookCount || 0} webhooks
                      </span>
                    </div>

                    <p className="text-xs text-gray-600 dark:text-gray-400 mb-4">
                      Created {app.createdAt ? formatRelativeTime(app.createdAt) : 'Unknown'}
                    </p>

                    <div className="flex gap-2">
                      <Link href={`/applications/${app.id}`} className="flex-1">
                        <Button variant="outline" size="sm" className="w-full">
                          <Settings className="w-4 h-4 mr-2" />
                          Manage
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
