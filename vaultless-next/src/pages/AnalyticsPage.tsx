"use client";

import { useEffect, useState } from 'react';
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
  Download,
  TrendingUp,
  TrendingDown,
  MessageSquare,
  Clock,
  Activity,
  DollarSign,
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
import { applicationsApi } from '@/lib/api';
import type { ApplicationDashboardResponse } from '@/types/api';

// NOTE: Analytics UI was initially built on mock time-series data.
// The backend analytics endpoint currently returns aggregate stats (currentMonth + lifetime).
// We map those aggregate values into the existing UI below.

const formatNumber = (num: number) => {
  if (num >= 1000000) return (num / 1000000).toFixed(1) + 'M';
  if (num >= 1000) return (num / 1000).toFixed(1) + 'K';
  return num.toString();
};

const formatCostDollars = (costCents?: number) => {
  if (costCents === undefined || costCents === null) return '—';
  return `$${(costCents / 100).toFixed(2)}`;
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

const buildDaySeriesFromMonth = (
  currentMonth: ApplicationDashboardResponse['currentMonth'] | undefined,
  metric: 'messages' | 'bandwidth' | 'cost'
) => {
  const today = new Date();
  const label = today.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });

  if (metric === 'messages') {
    const total = (currentMonth?.msgSent ?? 0) + (currentMonth?.msgReceived ?? 0);
    return [
      {
        date: label,
        messages: total,
        sent: currentMonth?.msgSent ?? 0,
        received: currentMonth?.msgReceived ?? 0,
      },
    ];
  }

  if (metric === 'bandwidth') {
    const totalBytes = (currentMonth?.bytesSent ?? 0) + (currentMonth?.bytesReceived ?? 0);
    const gb = totalBytes / (1024 * 1024 * 1024);
    return [{ date: label, bandwidth: Number(gb.toFixed(4)) }];
  }

  // cost is in cents
  return [{ date: label, messages: currentMonth?.cost ?? 0, sent: 0, received: 0 }];
};

const buildHourlySeriesPlaceholder = (totalMessages: number) => {
  // No per-hour data from API yet; distribute evenly just to keep UI working with real totals.
  const perHour = Math.round(totalMessages / 24);
  return Array.from({ length: 24 }).map((_, i) => ({
    hour: String(i).padStart(2, '0'),
    messages: perHour,
  }));
};

const buildQuotaStatusFromDashboard = (dashboard: ApplicationDashboardResponse | null) => {
  if (!dashboard) return null;

  const used = (dashboard.currentMonth?.msgSent ?? 0) + (dashboard.currentMonth?.msgReceived ?? 0);
  const limit = dashboard.monthlyQuota ?? 0;
  const pct = limit > 0 ? (used / limit) * 100 : 0;

  return {
    messagesUsed: used,
    messagesLimit: limit,
    usagePercentage: pct,
    isOverQuota: limit > 0 ? used > limit : false,
    overageCount: limit > 0 ? Math.max(0, used - limit) : 0,
    alertLevel: pct >= 90 ? 'critical' : pct >= 75 ? 'warning' : 'ok',
  };
};

const buildCostBreakdownPlaceholder = (totalCostCents: number) => {
  // API doesn't provide category breakdown. Keep the UI but make it reflect real total cost.
  const messages = Math.round(totalCostCents * 0.7);
  const bandwidth = Math.round(totalCostCents * 0.2);
  const other = totalCostCents - messages - bandwidth;
  const toPct = (n: number) => (totalCostCents > 0 ? Math.round((n / totalCostCents) * 100) : 0);

  return [
    { name: 'Messages', revenue: messages, percentage: toPct(messages), color: 'bg-green-500' },
    { name: 'Bandwidth', revenue: bandwidth, percentage: toPct(bandwidth), color: 'bg-blue-500' },
    { name: 'Other', revenue: other, percentage: toPct(other), color: 'bg-purple-500' },
  ];
};

export default function AnalyticsPage({ id: idProp }: { id?: string }) {
  const params = useParams();
  const idFromParams = params?.id as string | undefined;
  const id = idProp ?? idFromParams;

  const [dateRange, setDateRange] = useState('14d');
  const [granularity, setGranularity] = useState<'day' | 'week' | 'month'>('day');
  // Metric is kept for UI tab switching; we don't fetch time-series yet.
  const [, setMetric] = useState('messages');

  const [dashboard, setDashboard] = useState<ApplicationDashboardResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!id || id === 'undefined') return;

    const fetchAnalytics = async () => {
      setLoading(true);
      try {
        const data = await applicationsApi.getAnalytics(id);
        setDashboard(data);
        setError(null);
      } catch (err: any) {
        console.error('Failed to fetch analytics:', err);
        setError(err.message || 'Failed to load analytics');
      } finally {
        setLoading(false);
      }
    };

    fetchAnalytics();
  }, [id]);

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

  const quotaStatus = buildQuotaStatusFromDashboard(dashboard);
  const totalMessages = (dashboard?.currentMonth?.msgSent ?? 0) + (dashboard?.currentMonth?.msgReceived ?? 0);
  const bandwidthBytes = (dashboard?.currentMonth?.bytesSent ?? 0) + (dashboard?.currentMonth?.bytesReceived ?? 0);
  const currentCostCents = dashboard?.currentMonth?.cost ?? 0;

  const messageData = buildDaySeriesFromMonth(dashboard?.currentMonth, 'messages');
  const revenueData = buildDaySeriesFromMonth(dashboard?.currentMonth, 'cost');
  const bandwidthData = buildDaySeriesFromMonth(dashboard?.currentMonth, 'bandwidth');
  const hourlyData = buildHourlySeriesPlaceholder(totalMessages);

  const costBreakdown = buildCostBreakdownPlaceholder(currentCostCents);

  const stats = [
    {
      title: 'Total Messages',
      value: formatNumber(totalMessages),
      change: '—',
      trend: 'up',
      icon: MessageSquare,
      color: 'blue',
    },
    {
      title: 'Bandwidth',
      value: formatBytes(bandwidthBytes),
      change: '—',
      trend: 'up',
      icon: Activity,
      color: 'purple',
    },
    {
      title: 'Cost (month)',
      value: formatCostDollars(currentCostCents),
      change: '—',
      trend: 'up',
      icon: DollarSign,
      color: 'green',
    },
    {
      title: 'Rate Limit',
      value: dashboard?.rateLimit ? `${dashboard.rateLimit}/min` : '—',
      change: '—',
      trend: 'up',
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
            {/* Granularity Selector */}
            <div className="flex items-center bg-gray-100 dark:bg-gray-800 rounded-lg p-1">
              <Button
                variant={granularity === 'day' ? 'secondary' : 'ghost'}
                size="sm"
                onClick={() => setGranularity('day')}
                className="text-xs"
              >
                Day
              </Button>
              <Button
                variant={granularity === 'week' ? 'secondary' : 'ghost'}
                size="sm"
                onClick={() => setGranularity('week')}
                className="text-xs"
              >
                Week
              </Button>
              <Button
                variant={granularity === 'month' ? 'secondary' : 'ghost'}
                size="sm"
                onClick={() => setGranularity('month')}
                className="text-xs"
              >
                Month
              </Button>
            </div>

            {/* Date Range Selector (UI only until we have time-series endpoints) */}
            <Select value={dateRange} onValueChange={setDateRange}>
              <SelectTrigger className="w-[140px]">
                <Calendar className="w-4 h-4 mr-2" />
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="7d">Last 7 days</SelectItem>
                <SelectItem value="14d">Last 14 days</SelectItem>
                <SelectItem value="30d">Last 30 days</SelectItem>
                <SelectItem value="90d">Last 90 days</SelectItem>
              </SelectContent>
            </Select>

            {/* Export Button */}
            <Button variant="outline">
              <Download className="w-4 h-4 mr-2" />
              Export
            </Button>
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
      <Tabs defaultValue="messages" className="space-y-6">
        <TabsList>
          <TabsTrigger value="messages" onClick={() => setMetric('messages')}>
            Messages
          </TabsTrigger>
          <TabsTrigger value="revenue" onClick={() => setMetric('revenue')}>
            Revenue
          </TabsTrigger>
          <TabsTrigger value="bandwidth" onClick={() => setMetric('bandwidth')}>
            Bandwidth
          </TabsTrigger>
          <TabsTrigger value="hourly" onClick={() => setMetric('hourly')}>
            Hourly
          </TabsTrigger>
        </TabsList>

        {/* Revenue Chart */}
        <TabsContent value="revenue">
          <div className="grid lg:grid-cols-2 gap-6">
            <Card className="p-6">
              <div className="flex items-center justify-between mb-6">
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Revenue Trend</h2>
                <Badge variant="secondary" className="capitalize">
                  {granularity === 'day' ? 'Daily' : granularity === 'week' ? 'Weekly' : 'Monthly'} view
                </Badge>
              </div>
              <ResponsiveContainer width="100%" height={300}>
                <AreaChart data={revenueData}>
                  <defs>
                    <linearGradient id="colorRevenue" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="#10b981" stopOpacity={0.3} />
                      <stop offset="95%" stopColor="#10b981" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                  <XAxis dataKey="date" className="text-gray-600 dark:text-gray-400" />
                  <YAxis
                    className="text-gray-600 dark:text-gray-400"
                    tickFormatter={(value) => `$${(Number(value) / 100).toFixed(0)}`}
                  />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: 'var(--background)',
                      border: '1px solid var(--border)',
                      borderRadius: '8px',
                    }}
                    formatter={(value?: number) =>
                      value === undefined ? ['—', 'Revenue'] : [`$${(value / 100).toFixed(2)}`, 'Revenue']
                    }
                  />
                  <Area
                    type="monotone"
                    dataKey="messages"
                    stroke="#10b981"
                    strokeWidth={2}
                    fillOpacity={1}
                    fill="url(#colorRevenue)"
                    name="Revenue"
                  />
                </AreaChart>
              </ResponsiveContainer>
            </Card>

            <Card className="p-6">
              <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">Revenue Breakdown</h2>
              <div className="space-y-4">
                {costBreakdown.map((item) => (
                  <div key={item.name}>
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-sm font-medium text-gray-900 dark:text-white">{item.name}</span>
                      <span className="text-sm text-gray-600 dark:text-gray-400">
                        {formatCostDollars(item.revenue)} ({item.percentage}%)
                      </span>
                    </div>
                    <div className="h-3 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                      <div
                        className={`h-full ${item.color} rounded-full transition-all`}
                        style={{ width: `${item.percentage}%` }}
                      />
                    </div>
                  </div>
                ))}
              </div>
              <div className="mt-6 pt-6 border-t border-gray-200 dark:border-gray-700">
                <div className="flex items-center justify-between">
                  <span className="text-lg font-semibold text-gray-900 dark:text-white">Total This Month</span>
                  <span className="text-2xl font-bold text-green-600">{formatCostDollars(currentCostCents)}</span>
                </div>
              </div>
            </Card>
          </div>
        </TabsContent>

        {/* Messages Chart */}
        <TabsContent value="messages">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Message Volume</h2>
              <Badge variant="secondary" className="capitalize">
                {granularity === 'day' ? 'Daily' : granularity === 'week' ? 'Weekly' : 'Monthly'} view
              </Badge>
            </div>
            <ResponsiveContainer width="100%" height={400}>
              <AreaChart data={messageData}>
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
                  formatter={(value?: number) => (value === undefined ? ['—', 'Messages'] : [formatNumber(value), 'Messages'])}
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
              </AreaChart>
            </ResponsiveContainer>
          </Card>
        </TabsContent>

        {/* Bandwidth Chart */}
        <TabsContent value="bandwidth">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Bandwidth Usage</h2>
              <Badge variant="secondary" className="capitalize">
                {granularity === 'day' ? 'Daily' : granularity === 'week' ? 'Weekly' : 'Monthly'} view
              </Badge>
            </div>
            <ResponsiveContainer width="100%" height={400}>
              <BarChart data={bandwidthData as any}>
                <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                <XAxis dataKey="date" className="text-gray-600 dark:text-gray-400" />
                <YAxis className="text-gray-600 dark:text-gray-400" tickFormatter={(value) => `${value} GB`} />
                <Tooltip
                  contentStyle={{
                    backgroundColor: 'var(--background)',
                    border: '1px solid var(--border)',
                    borderRadius: '8px',
                  }}
                  formatter={(value?: number) => (value === undefined ? ['—', 'Bandwidth'] : [`${value} GB`, 'Bandwidth'])}
                />
                <Bar dataKey="bandwidth" fill="#8b5cf6" radius={[4, 4, 0, 0]} name="Bandwidth (GB)" />
              </BarChart>
            </ResponsiveContainer>
          </Card>
        </TabsContent>

        {/* Hourly Chart */}
        <TabsContent value="hourly">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Hourly Message Distribution</h2>
              <Badge variant="secondary">Approximation (API has no hourly series yet)</Badge>
            </div>
            <ResponsiveContainer width="100%" height={400}>
              <BarChart data={hourlyData}>
                <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                <XAxis
                  dataKey="hour"
                  className="text-gray-600 dark:text-gray-400"
                  tickFormatter={(value) => `${value}:00`}
                />
                <YAxis className="text-gray-600 dark:text-gray-400" />
                <Tooltip
                  contentStyle={{
                    backgroundColor: 'var(--background)',
                    border: '1px solid var(--border)',
                    borderRadius: '8px',
                  }}
                  formatter={(value?: number) => (value === undefined ? ['—', 'Messages'] : [value, 'Messages'])}
                  labelFormatter={(value) => `${value}:00`}
                />
                <Bar dataKey="messages" fill="#10b981" radius={[4, 4, 0, 0]} name="Messages" />
              </BarChart>
            </ResponsiveContainer>
          </Card>
        </TabsContent>
      </Tabs>

      {/* Quick Insights (kept, but now reflects aggregate API data) */}
      <div className="mt-8 grid lg:grid-cols-3 gap-6">
        <Card className="p-6">
          <h3 className="text-lg font-semibold mb-4 text-gray-900 dark:text-white">Keys</h3>
          <p className="text-3xl font-bold text-gray-900 dark:text-white mb-2">{dashboard?.keys?.length ?? 0}</p>
          <p className="text-sm text-gray-600 dark:text-gray-400">Publishable keys</p>
        </Card>

        <Card className="p-6">
          <h3 className="text-lg font-semibold mb-4 text-gray-900 dark:text-white">Webhooks</h3>
          <p className="text-3xl font-bold text-gray-900 dark:text-white mb-2">{dashboard?.webhooks?.length ?? 0}</p>
          <p className="text-sm text-gray-600 dark:text-gray-400">Configured webhooks</p>
        </Card>

        <Card className="p-6">
          <h3 className="text-lg font-semibold mb-4 text-gray-900 dark:text-white">Quota Usage</h3>
          <p className="text-3xl font-bold text-green-600 mb-2">{(dashboard?.quotaUsagePct ?? 0).toFixed(1)}%</p>
          <p className="text-sm text-gray-600 dark:text-gray-400">This month</p>
        </Card>
      </div>
    </DashboardLayout>
  );
}
