"use client";

import { useEffect, useState, useCallback } from 'react';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import { motion } from 'motion/react';
import {
  Line,
  AreaChart,
  Area,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
} from 'recharts';
import {
  Calendar,
  TrendingUp,
  TrendingDown,
  MessageSquare,
  Clock,
  Activity,
  ArrowLeft,
  AlertCircle,
  ChevronRight,
  ShieldCheck,
  Loader2,
} from 'lucide-react';
import DashboardLayout from '../components/layout/DashboardLayout';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Progress } from '../components/ui/progress';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs';
import { applicationsApi } from '@/lib/api/applications';
import type { ApplicationDashboardResponse, ApplicationChartData, UsageChartPoint } from '@/types/api';

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

const formatNumber = (num: number) => {
  if (num >= 1000000) return (num / 1000000).toFixed(1) + 'M';
  if (num >= 1000) return (num / 1000).toFixed(1) + 'K';
  return num.toString();
};

const formatBytes = (bytes?: number) => {
  if (bytes === undefined || bytes === null) return '—';
  const gb = bytes / (1024 * 1024 * 1024);
  if (gb >= 1) return `${gb.toFixed(2)} GB`;
  const mb = bytes / (1024 * 1024);
  if (mb >= 1) return `${mb.toFixed(2)} MB`;
  const kb = bytes / 1024;
  if (kb >= 1) return `${kb.toFixed(2)} KB`;
  return `${bytes} B`;
};

const formatDate = (timestamp: string) => {
  const date = new Date(timestamp);
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
};

// Date range helpers
const getDateRange = (range: string) => {
  const end = new Date();
  const start = new Date();

  switch (range) {
    case '7d':
      start.setDate(start.getDate() - 7);
      break;
    case '14d':
      start.setDate(start.getDate() - 14);
      break;
    case '30d':
      start.setDate(start.getDate() - 30);
      break;
    case '60d':
      start.setDate(start.getDate() - 60);
      break;
    case '90d':
      start.setDate(start.getDate() - 90);
      break;
  }

  return {
    start: start.toISOString().split('T')[0],
    end: end.toISOString().split('T')[0],
  };
};

// Get label for date range
const getDateRangeLabel = (range: string) => {
  const labels: Record<string, string> = {
    '7d': 'Past 7 days',
    '14d': 'Past 2 weeks',
    '30d': 'Past month',
    '60d': 'Past 2 months',
    '90d': 'Past 3 months',
  };
  return labels[range] || range;
};

// Get recommended granularity for date range
const getRecommendedGranularity = (range: string): 'daily' | 'weekly' => {
  // Backend limits: daily max 100 buckets, weekly max 160 buckets
  if (['7d', '14d', '30d'].includes(range)) return 'daily';
  return 'weekly'; // 60d, 90d look better with weekly aggregation
};

// ============================================================================
// MAIN COMPONENT
// ============================================================================

export default function AnalyticsPage({ id: idProp }: { id?: string }) {
  const params = useParams();
  const idFromParams = params?.id as string | undefined;
  const id = idProp ?? idFromParams;

  // UI state
  const [dateRange, setDateRange] = useState('14d');
  const [granularity, setGranularity] = useState<'daily' | 'weekly'>('daily');
  const [activeTab, setActiveTab] = useState('messages');

  // Auto-switch granularity when date range changes
  useEffect(() => {
    const recommended = getRecommendedGranularity(dateRange);
    setGranularity(recommended);
  }, [dateRange]);
  
  // Data state
  const [dashboard, setDashboard] = useState<ApplicationDashboardResponse | null>(null);
  const [chartData, setChartData] = useState<ApplicationChartData | null>(null);
  const [loading, setLoading] = useState(true);
  const [chartLoading, setChartLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Fetch dashboard data
  const fetchDashboard = useCallback(async () => {
    if (!id || id === 'undefined') return;
    
    try {
      const data = await applicationsApi.getAnalytics(id);
      setDashboard(data);
      setError(null);
    } catch (err: any) {
      console.error('Failed to fetch analytics:', err);
      setError(err.message || 'Failed to load analytics');
    }
  }, [id]);

  // Fetch chart data
  const fetchChartData = useCallback(async () => {
    if (!id || id === 'undefined') return;

    setChartLoading(true);
    try {
      const { start, end } = getDateRange(dateRange);
      const metric = activeTab;

      const data = await applicationsApi.getChartData(id, {
        granularity,
        metric: metric as any,
        start,
        end,
        includeTrends: true,
      });
      setChartData(data);
    } catch (err: any) {
      console.error('Failed to fetch chart data:', err);
      // Don't show error for chart failures, just use empty data
    } finally {
      setChartLoading(false);
    }
  }, [id, dateRange, granularity, activeTab]);

  useEffect(() => {
    if (!id || id === 'undefined') return;

    const init = async () => {
      setLoading(true);
      await fetchDashboard();
      setLoading(false);
    };

    init();
  }, [id, fetchDashboard]);

  useEffect(() => {
    if (dashboard) {
      fetchChartData();
    }
  }, [dateRange, granularity, activeTab, fetchChartData]);

  if (!id || id === 'undefined') {
    return (
      <DashboardLayout>
        <div className="mb-6">
          <Link
            href="/applications"
            className="inline-flex items-center text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white mb-4"
          >
            <ArrowLeft className="w-4 h-4 mr-2" />
            Back to Apps
          </Link>
          <Card className="p-6">
            <p className="text-gray-600 dark:text-gray-400">Missing application id.</p>
          </Card>
        </div>
      </DashboardLayout>
    );
  }

  if (loading) {
    return (
      <DashboardLayout>
        <div className="flex items-center justify-center py-24">
          <Loader2 className="w-8 h-8 animate-spin text-blue-600" />
          <span className="ml-3 text-gray-600 dark:text-gray-400">Loading analytics...</span>
        </div>
      </DashboardLayout>
    );
  }

  if (error && !dashboard) {
    return (
      <DashboardLayout>
        <div className="mb-6">
          <Link
            href={`/applications/${id}`}
            className="inline-flex items-center text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white mb-4"
          >
            <ArrowLeft className="w-4 h-4 mr-2" />
            Back to Application
          </Link>
          <Card className="p-6 border-red-200 bg-red-50 dark:bg-red-900/10 text-center">
            <p className="text-red-600 dark:text-red-400">{error}</p>
          </Card>
        </div>
      </DashboardLayout>
    );
  }

  const appName = dashboard?.name || 'Application';
  const tierText = dashboard?.tier ? String(dashboard.tier) : '';
  const tierNormalized = tierText.toLowerCase();

  // Build quota status from dashboard data
  const quotaStatus = dashboard ? {
    messagesUsed: (dashboard.currentMonth?.msgSent ?? 0) + (dashboard.currentMonth?.msgReceived ?? 0),
    messagesLimit: dashboard.monthlyQuota ?? 0,
    usagePercentage: dashboard.monthlyQuota 
      ? ((dashboard.currentMonth?.msgSent ?? 0) + (dashboard.currentMonth?.msgReceived ?? 0)) / dashboard.monthlyQuota * 100
      : 0,
    isOverQuota: dashboard.monthlyQuota 
      ? ((dashboard.currentMonth?.msgSent ?? 0) + (dashboard.currentMonth?.msgReceived ?? 0)) > dashboard.monthlyQuota
      : false,
    overageCount: dashboard.monthlyQuota
      ? Math.max(0, ((dashboard.currentMonth?.msgSent ?? 0) + (dashboard.currentMonth?.msgReceived ?? 0)) - dashboard.monthlyQuota)
      : 0,
    alertLevel: dashboard.quotaUsagePct >= 90 ? 'critical' : dashboard.quotaUsagePct >= 75 ? 'warning' : 'ok',
  } : null;

  const totalMessages = (dashboard?.currentMonth?.msgSent ?? 0) + (dashboard?.currentMonth?.msgReceived ?? 0);
  const bandwidthBytes = (dashboard?.currentMonth?.bytesSent ?? 0) + (dashboard?.currentMonth?.bytesReceived ?? 0);

  // Get trend data from chart
  const messagesTrend = chartData?.trends?.trendDirection ?? 'up';
  const messagesChange = chartData?.trends?.changePercent ?? 0;

  // Transform chart data for Recharts
  const getChartData = () => {
    if (!chartData?.dataPoints) return [];

    return chartData.dataPoints.map((point: UsageChartPoint) => {
      const base: any = {
        date: formatDate(point.timestamp),
        timestamp: point.timestamp,
      };

      if (activeTab === 'messages') {
        base.messages = (point.messagesSent ?? 0) + (point.messagesReceived ?? 0);
        base.sent = point.messagesSent ?? 0;
        base.received = point.messagesReceived ?? 0;
        base.proofs = point.proofsVerified ?? 0;
        base.rateHits = point.rateLimitHits ?? 0;
      } else if (activeTab === 'bandwidth') {
        base.bytesSent = point.bytesSent ?? 0;
        base.bytesReceived = point.bytesReceived ?? 0;
        base.bytesStored = point.bytesStored ?? 0;
        base.bandwidth = ((point.bytesSent ?? 0) + (point.bytesReceived ?? 0)) / (1024 * 1024 * 1024);
      }

      return base;
    });
  };

  const chartDataTransformed = getChartData();

  // Stats cards data
  const stats = [
    {
      title: 'Total Messages',
      value: formatNumber(totalMessages),
      change: messagesChange !== 0 ? `${messagesChange > 0 ? '+' : ''}${messagesChange.toFixed(1)}%` : '—',
      trend: messagesTrend as 'up' | 'down',
      icon: MessageSquare,
      color: 'blue',
    },
    {
      title: 'Bandwidth',
      value: formatBytes(bandwidthBytes),
      change: '—',
      trend: 'up' as const,
      icon: Activity,
      color: 'purple',
    },
    {
      title: 'Rate Limit',
      value: dashboard?.rateLimit ? `${dashboard.rateLimit}/min` : '—',
      change: '—',
      trend: 'up' as const,
      icon: Clock,
      color: 'orange',
    },
  ];

  return (
    <DashboardLayout>
      {/* Header */}
      <div className="mb-6">
        <Link
          href={`/applications/${id}`}
          className="inline-flex items-center text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white mb-4"
        >
          <ArrowLeft className="w-4 h-4 mr-2" />
          Back to {appName}
        </Link>
        <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
          <div>
            <div className="flex items-center gap-3 mb-2">
              <h1 className="text-3xl font-bold text-gray-900 dark:text-white">{appName}</h1>
              {tierText && <Badge>{tierText}</Badge>}
            </div>
            <p className="text-gray-600 dark:text-gray-400">Analytics for this application</p>
          </div>
          <div className="flex items-center gap-3">
            {/* Date Range Selector */}
            <div className="flex items-center gap-2">
              <Calendar className="w-4 h-4 text-gray-500" />
              <Select value={dateRange} onValueChange={setDateRange}>
                <SelectTrigger className="w-[150px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="7d">Past 7 days</SelectItem>
                  <SelectItem value="14d">Past 2 weeks</SelectItem>
                  <SelectItem value="30d">Past month</SelectItem>
                  <SelectItem value="60d">Past 2 months</SelectItem>
                  <SelectItem value="90d">Past 3 months</SelectItem>
                </SelectContent>
              </Select>
            </div>

            {/* Granularity Badge (auto-selected) */}
            <Badge variant="outline" className="hidden sm:inline-flex">
              {granularity === 'daily' ? '📊 Daily' : '📈 Weekly'} view
            </Badge>

            {/* Remove export button - not implemented */}
          </div>
        </div>
      </div>

      {/* Quota Status Section */}
      {quotaStatus && (
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6 mb-8">
          <Card className="lg:col-span-2 p-6">
            <div className="flex items-center justify-between mb-6">
              <div>
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white flex items-center gap-2">
                  Monthly Quota Usage
                  {quotaStatus.alertLevel === 'critical' || quotaStatus.isOverQuota ? (
                    <AlertCircle className="w-5 h-5 text-red-500" />
                  ) : quotaStatus.alertLevel === 'warning' ? (
                    <AlertCircle className="w-5 h-5 text-amber-500" />
                  ) : (
                    <ShieldCheck className="w-5 h-5 text-green-500" />
                  )}
                </h2>
                <p className="text-sm text-gray-600 dark:text-gray-400">Current billing period</p>
              </div>
              <div className="text-right">
                <span className="text-2xl font-bold text-gray-900 dark:text-white">
                  {quotaStatus.usagePercentage.toFixed(1)}%
                </span>
                <p className="text-xs text-gray-600 dark:text-gray-400 uppercase tracking-wider font-semibold">Used</p>
              </div>
            </div>

            <div className="space-y-4">
              <div
                className={
                  quotaStatus.usagePercentage >= 90
                    ? 'bg-red-500'
                    : quotaStatus.usagePercentage >= 75
                      ? 'bg-amber-500'
                      : 'bg-blue-600'
                }
              >
                <Progress value={quotaStatus.usagePercentage} className="h-3" />
              </div>
              <div className="flex justify-between text-sm">
                <span className="text-gray-600 dark:text-gray-400">
                  <span className="font-semibold text-gray-900 dark:text-white">
                    {formatNumber(quotaStatus.messagesUsed)}
                  </span>{' '}
                  messages used
                </span>
                <span className="text-gray-600 dark:text-gray-400">
                  Quota:{' '}
                  <span className="font-semibold text-gray-900 dark:text-white">
                    {formatNumber(quotaStatus.messagesLimit)}
                  </span>
                </span>
              </div>
            </div>

            {quotaStatus.isOverQuota && (
              <div className="mt-4 p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-900/50 rounded-lg flex items-start gap-3">
                <AlertCircle className="w-5 h-5 text-red-600 mt-0.5" />
                <div>
                  <p className="text-sm font-semibold text-red-900 dark:text-red-400">Quota Exceeded</p>
                  <p className="text-xs text-red-700 dark:text-red-300">
                    You are {formatNumber(quotaStatus.overageCount)} messages over your monthly limit.
                  </p>
                </div>
              </div>
            )}
          </Card>

          <Card className="p-6 flex flex-col justify-between">
            <div>
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white mb-2">Plan Limit</h2>
              <div className="flex items-center gap-2 mb-4">
                <Badge className="px-3 py-1 text-sm">{tierText || '—'}</Badge>
                <span className="text-sm text-gray-600 dark:text-gray-400">Subscription</span>
              </div>
              <ul className="space-y-3">
                {[
                  { label: 'Monthly Messages', value: formatNumber(quotaStatus.messagesLimit) },
                  { label: 'Analytics History', value: tierNormalized === 'pro' ? '90 days' : '7 days' },
                  { label: 'Rate Limit', value: dashboard?.rateLimit ? `${dashboard.rateLimit}/min` : '—' },
                ].map((item) => (
                  <li key={item.label} className="flex justify-between text-sm">
                    <span className="text-gray-600 dark:text-gray-400">{item.label}</span>
                    <span className="font-medium text-gray-900 dark:text-white">{item.value}</span>
                  </li>
                ))}
              </ul>
            </div>
            <Button variant="outline" className="w-full mt-6 group">
              Upgrade Plan
              <ChevronRight className="w-4 h-4 ml-2 transition-transform group-hover:translate-x-1" />
            </Button>
          </Card>
        </div>
      )}

      {/* Stats Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6 mb-8">
        {stats.map((stat, index) => (
          <motion.div
            key={stat.title}
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: index * 0.1 }}
          >
            <Card className="p-6">
              <div className="flex items-center justify-between mb-2">
                <span className="text-sm font-medium text-gray-600 dark:text-gray-400">{stat.title}</span>
                <div
                  className={`w-10 h-10 rounded-lg flex items-center justify-center bg-${stat.color}-100 dark:bg-${stat.color}-900/20`}
                >
                  <stat.icon className={`w-5 h-5 text-${stat.color}-600`} />
                </div>
              </div>
              <div className="flex items-end gap-2">
                <span className="text-3xl font-bold text-gray-900 dark:text-white">{stat.value}</span>
                <span
                  className={`text-sm flex items-center mb-1 ${stat.trend === 'up' ? 'text-green-600' : 'text-red-600'}`}
                >
                  {stat.trend === 'up' ? (
                    <TrendingUp className="w-4 h-4 mr-1" />
                  ) : (
                    <TrendingDown className="w-4 h-4 mr-1" />
                  )}
                  {stat.change}
                </span>
              </div>
            </Card>
          </motion.div>
        ))}
      </div>

      {/* Main Charts */}
      <Tabs value={activeTab} onValueChange={setActiveTab} className="space-y-6">
        <TabsList>
          <TabsTrigger value="messages">Messages</TabsTrigger>
          <TabsTrigger value="bandwidth">Bandwidth</TabsTrigger>
        </TabsList>

        {/* Messages Chart */}
        <TabsContent value="messages" className="space-y-6">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Message Volume</h2>
              <Badge variant="secondary" className="capitalize">
                {granularity === 'daily' ? 'Daily' : 'Weekly'} view
              </Badge>
            </div>

            {chartLoading ? (
              <div className="flex items-center justify-center h-[400px]">
                <Loader2 className="w-8 h-8 animate-spin text-blue-600" />
              </div>
            ) : (
              <ResponsiveContainer width="100%" height={400}>
                <AreaChart data={chartDataTransformed}>
                  <defs>
                    <linearGradient id="colorMessages" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="#2563eb" stopOpacity={0.3} />
                      <stop offset="95%" stopColor="#2563eb" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                  <XAxis dataKey="date" className="text-gray-600 dark:text-gray-400" />
                  <YAxis className="text-gray-600 dark:text-gray-400" tickFormatter={formatNumber} />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: 'var(--background)',
                      border: '1px solid var(--border)',
                      borderRadius: '8px',
                    }}
                    formatter={(value?: number, name?: string) => {
                      if (value === undefined) return ['—', name || ''];
                      if (name === 'sent') return [formatNumber(value), 'Sent'];
                      if (name === 'received') return [formatNumber(value), 'Received'];
                      if (name === 'proofs') return [formatNumber(value), 'Proofs Verified'];
                      return [formatNumber(value), 'Total Messages'];
                    }}
                  />
                  <Legend />
                  <Area
                    type="monotone"
                    dataKey="messages"
                    stroke="#2563eb"
                    strokeWidth={2}
                    fillOpacity={1}
                    fill="url(#colorMessages)"
                    name="Total Messages"
                  />
                  <Line type="monotone" dataKey="sent" stroke="#10b981" strokeWidth={2} dot={false} name="Sent" />
                  <Line
                    type="monotone"
                    dataKey="received"
                    stroke="#f59e0b"
                    strokeWidth={2}
                    dot={false}
                    name="Received"
                  />
                  <Line
                    type="monotone"
                    dataKey="proofs"
                    stroke="#8b5cf6"
                    strokeWidth={2}
                    dot={false}
                    name="Proofs Verified"
                  />
                </AreaChart>
              </ResponsiveContainer>
            )}
          </Card>
        </TabsContent>

        {/* Bandwidth Chart */}
        <TabsContent value="bandwidth">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Bandwidth Usage</h2>
              <Badge variant="secondary" className="capitalize">
                {granularity === 'daily' ? 'Daily' : 'Weekly'} view
              </Badge>
            </div>

            {chartLoading ? (
              <div className="flex items-center justify-center h-[400px]">
                <Loader2 className="w-8 h-8 animate-spin text-blue-600" />
              </div>
            ) : (
              <ResponsiveContainer width="100%" height={400}>
                <BarChart data={chartDataTransformed}>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                  <XAxis dataKey="date" className="text-gray-600 dark:text-gray-400" />
                  <YAxis className="text-gray-600 dark:text-gray-400" tickFormatter={(value) => `${(value / (1024 * 1024 * 1024)).toFixed(2)} GB`} />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: 'var(--background)',
                      border: '1px solid var(--border)',
                      borderRadius: '8px',
                    }}
                    formatter={(value?: number, name?: string) => {
                      if (value === undefined) return ['—', name || ''];
                      const gb = (value / (1024 * 1024 * 1024)).toFixed(2);
                      if (name === 'bytesSent') return [`${gb} GB`, 'Bytes Sent'];
                      if (name === 'bytesReceived') return [`${gb} GB`, 'Bytes Received'];
                      if (name === 'bytesStored') return [`${gb} GB`, 'Bytes Stored'];
                      return [`${gb} GB`, 'Total Bandwidth'];
                    }}
                  />
                  <Legend />
                  <Bar dataKey="bytesSent" fill="#8b5cf6" radius={[4, 4, 0, 0]} name="Bytes Sent" stackId="a" />
                  <Bar dataKey="bytesReceived" fill="#06b6d4" radius={[4, 4, 0, 0]} name="Bytes Received" stackId="a" />
                  <Bar dataKey="bytesStored" fill="#3b82f6" radius={[4, 4, 0, 0]} name="Bytes Stored" stackId="a" />
                </BarChart>
              </ResponsiveContainer>
            )}
          </Card>
        </TabsContent>
      </Tabs>
    </DashboardLayout>
  );
}
