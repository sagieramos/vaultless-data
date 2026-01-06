"use client";
import { useState, useEffect } from 'react';
import Link from 'next/link';
import { motion } from 'motion/react';
import { Plus, BarChart3, Settings, Server, MessageSquare, Loader2 } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import DashboardLayout from '../components/layout/DashboardLayout';
import { useRequireAuth } from '@/contexts/AuthContext';
import { applicationsApi } from '@/lib/api';
import type { ApplicationListParams } from '@/types/api';
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
  useRequireAuth();
  const [applications, setApplications] = useState<any[]>([]);
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
    fetchApplications();
  }, [filters, pagination.page, pagination.pageSize]);

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
            {applications.map((app, index) => (
              <motion.div
                key={app.id}
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ delay: index * 0.05 }}
              >
                <Card className="p-6 hover:shadow-lg transition-shadow h-full flex flex-col">
                  <div className="flex items-start justify-between mb-3">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 mb-1">
                        <div className="w-8 h-8 bg-blue-100 dark:bg-blue-900/30 rounded-lg flex items-center justify-center flex-shrink-0">
                          <MessageSquare className="w-4 h-4 text-blue-600" />
                        </div>
                        <h3 className="font-semibold text-lg text-gray-900 dark:text-white truncate">
                          {app.name}
                        </h3>
                        {app.is_active && (
                          <div className="w-2 h-2 rounded-full bg-green-500 flex-shrink-0" />
                        )}
                      </div>
                      <p className="text-sm text-gray-600 dark:text-gray-400 line-clamp-2">
                        {app.description || 'No description'}
                      </p>
                    </div>
                    <div className="flex flex-col items-end gap-1 flex-shrink-0 ml-2">
                      <Badge
                        variant={
                          app.tier === 'pro' || app.tier === 'enterprise'
                            ? 'default'
                            : 'secondary'
                        }
                      >
                        {app.tier || 'Free'}
                      </Badge>
                    </div>
                  </div>

                  <div className="mt-auto">
                    <p className="text-xs text-gray-600 dark:text-gray-400 mb-3">
                      Created {new Date(app.created_at).toLocaleDateString()}
                    </p>

                    {/* Quota Usage */}
                    {app.quotaUsagePercentage && (
                      <div className="mb-3">
                        <div className="flex justify-between text-xs text-gray-600 dark:text-gray-400 mb-1">
                          <span>Quota Usage</span>
                          <span>{app.quotaUsagePercentage}</span>
                        </div>
                        <div className="h-1.5 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                          <div
                            className={`h-full rounded-full transition-all ${
                              parseFloat(app.quotaUsagePercentage) >= 90
                                ? 'bg-red-500'
                                : parseFloat(app.quotaUsagePercentage) >= 70
                                  ? 'bg-yellow-500'
                                  : 'bg-green-500'
                            }`}
                            style={{ width: app.quotaUsagePercentage }}
                          />
                        </div>
                      </div>
                    )}

                    {/* Actions */}
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
                  </div>
                </Card>
              </motion.div>
            ))}
          </div>

          {/* Pagination */}
          {renderPagination()}
        </>
      )}
    </DashboardLayout>
  );
}
