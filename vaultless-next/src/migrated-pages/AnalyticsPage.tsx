"use client";
import { useState } from 'react';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import { motion } from 'motion/react';
import {
  LineChart, Line, AreaChart, Area, BarChart, Bar,
  XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Legend
} from 'recharts';
import {
  Calendar, Download, TrendingUp, TrendingDown,
  MessageSquare, Clock, Activity, DollarSign, Users, CreditCard, ArrowLeft
} from 'lucide-react';
import DashboardLayout from '../components/layout/DashboardLayout';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs';

// Mock data
const messageData = [
  { date: 'Jan 1', messages: 2400, sent: 1200, received: 1200 },
  { date: 'Jan 2', messages: 1398, sent: 700, received: 698 },
  { date: 'Jan 3', messages: 9800, sent: 4900, received: 4900 },
  { date: 'Jan 4', messages: 3908, sent: 1950, received: 1958 },
  { date: 'Jan 5', messages: 4800, sent: 2400, received: 2400 },
  { date: 'Jan 6', messages: 3800, sent: 1900, received: 1900 },
  { date: 'Jan 7', messages: 4300, sent: 2150, received: 2150 },
  { date: 'Jan 8', messages: 5100, sent: 2550, received: 2550 },
  { date: 'Jan 9', messages: 4200, sent: 2100, received: 2100 },
  { date: 'Jan 10', messages: 5900, sent: 2950, received: 2950 },
  { date: 'Jan 11', messages: 6100, sent: 3050, received: 3050 },
  { date: 'Jan 12', messages: 5600, sent: 2800, received: 2800 },
  { date: 'Jan 13', messages: 6400, sent: 3200, received: 3200 },
  { date: 'Jan 14', messages: 7100, sent: 3550, received: 3550 },
];

const bandwidthData = [
  { date: 'Jan 1', bandwidth: 2.4 },
  { date: 'Jan 2', bandwidth: 1.8 },
  { date: 'Jan 3', bandwidth: 5.2 },
  { date: 'Jan 4', bandwidth: 3.1 },
  { date: 'Jan 5', bandwidth: 4.2 },
  { date: 'Jan 6', bandwidth: 3.8 },
  { date: 'Jan 7', bandwidth: 4.5 },
  { date: 'Jan 8', bandwidth: 5.1 },
  { date: 'Jan 9', bandwidth: 4.2 },
  { date: 'Jan 10', bandwidth: 6.2 },
  { date: 'Jan 11', bandwidth: 5.8 },
  { date: 'Jan 12', bandwidth: 5.4 },
  { date: 'Jan 13', bandwidth: 6.8 },
  { date: 'Jan 14', bandwidth: 7.2 },
];

const costBreakdown = [
  { name: 'Messages', cost: 2850, percentage: 63 },
  { name: 'Bandwidth', cost: 1023, percentage: 23 },
  { name: 'Storage', cost: 450, percentage: 10 },
  { name: 'Other', cost: 200, percentage: 4 },
];

const hourlyData = [
  { hour: '00', messages: 120 },
  { hour: '01', messages: 80 },
  { hour: '02', messages: 60 },
  { hour: '03', messages: 40 },
  { hour: '04', messages: 30 },
  { hour: '05', messages: 50 },
  { hour: '06', messages: 120 },
  { hour: '07', messages: 250 },
  { hour: '08', messages: 480 },
  { hour: '09', messages: 620 },
  { hour: '10', messages: 780 },
  { hour: '11', messages: 850 },
  { hour: '12', messages: 720 },
  { hour: '13', messages: 680 },
  { hour: '14', messages: 750 },
  { hour: '15', messages: 820 },
  { hour: '16', messages: 900 },
  { hour: '17', messages: 780 },
  { hour: '18', messages: 620 },
  { hour: '19', messages: 480 },
  { hour: '20', messages: 380 },
  { hour: '21', messages: 320 },
  { hour: '22', messages: 250 },
  { hour: '23', messages: 180 },
];

// Mock applications data - in real app this would come from API
const applicationsData: Record<string, { name: string; tier: string }> = {
  '1': { name: 'Production API', tier: 'Pro' },
  '2': { name: 'Staging Environment', tier: 'Free' },
  '3': { name: 'Mobile App Backend', tier: 'Pro' },
};

export default function AnalyticsPage() {
  const params = useParams();
  const id = params?.id as string | undefined;
  const [dateRange, setDateRange] = useState('14d');
  const [metric, setMetric] = useState('messages');
  const [granularity, setGranularity] = useState<'day' | 'week' | 'month'>('day');

  // Get current application data
  const appData = id ? applicationsData[id] : null;
  const appName = appData?.name || 'Application';
  const appTier = appData?.tier || '';

  const stats = [
    {
      title: 'Total Messages',
      value: '78,542',
      change: '+12.5%',
      trend: 'up',
      icon: MessageSquare,
      color: 'blue'
    },
    {
      title: 'Active Subscribers',
      value: '142',
      change: '+12%',
      trend: 'up',
      icon: Users,
      color: 'purple'
    },
    {
      title: 'Revenue',
      value: '$1,284',
      change: '+24%',
      trend: 'up',
      icon: CreditCard,
      color: 'green'
    },
    {
      title: 'Avg Response Time',
      value: '12ms',
      change: '-8%',
      trend: 'up',
      icon: Clock,
      color: 'orange'
    }
  ];

  const formatNumber = (num: number) => {
    if (num >= 1000000) return (num / 1000000).toFixed(1) + 'M';
    if (num >= 1000) return (num / 1000).toFixed(1) + 'K';
    return num.toString();
  };

  return (
    <DashboardLayout>
      {/* Header */}
      <div className="mb-6">
        <Link href={`/applications/${id}`} className="inline-flex items-center text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white mb-4">
          <ArrowLeft className="w-4 h-4 mr-2" />
          Back to {appName}
        </Link>
        <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
          <div>
            <div className="flex items-center gap-3 mb-2">
              <h1 className="text-3xl font-bold text-gray-900 dark:text-white">
                {appName}
              </h1>
              {appTier && <Badge>{appTier}</Badge>}
            </div>
            <p className="text-gray-600 dark:text-gray-400">
              Analytics for this application
            </p>
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

            {/* Date Range Selector */}
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
                <span className="text-sm font-medium text-gray-600 dark:text-gray-400">
                  {stat.title}
                </span>
                <div className={`w-10 h-10 rounded-lg flex items-center justify-center bg-${stat.color}-100 dark:bg-${stat.color}-900/20`}>
                  <stat.icon className={`w-5 h-5 text-${stat.color}-600`} />
                </div>
              </div>
              <div className="flex items-end gap-2">
                <span className="text-3xl font-bold text-gray-900 dark:text-white">
                  {stat.value}
                </span>
                <span className={`text-sm flex items-center mb-1 ${
                  stat.trend === 'up' ? 'text-green-600' : 'text-red-600'
                }`}>
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
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">
                  Revenue Trend
                </h2>
                <Badge variant="secondary" className="capitalize">
                  {granularity === 'day' ? 'Daily' : granularity === 'week' ? 'Weekly' : 'Monthly'} view
                </Badge>
              </div>
              <ResponsiveContainer width="100%" height={300}>
                <AreaChart data={messageData}>
                  <defs>
                    <linearGradient id="colorRevenue" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="#10b981" stopOpacity={0.3}/>
                      <stop offset="95%" stopColor="#10b981" stopOpacity={0}/>
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                  <XAxis dataKey="date" className="text-gray-600 dark:text-gray-400" />
                  <YAxis
                    className="text-gray-600 dark:text-gray-400"
                    tickFormatter={(value) => `$${(value / 100).toFixed(0)}`}
                  />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: 'var(--background)',
                      border: '1px solid var(--border)',
                      borderRadius: '8px'
                    }}
                    formatter={(value?: number) => value === undefined ? ['—', 'Revenue'] : [`$${(value / 100).toFixed(2)}`, 'Revenue']}
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
              <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">
                Revenue Breakdown
              </h2>
              <div className="space-y-4">
                {[
                  { name: 'Subscriptions', revenue: 892, percentage: 69, color: 'bg-green-500' },
                  { name: 'Usage-based', revenue: 245, percentage: 19, color: 'bg-blue-500' },
                  { name: 'One-time', revenue: 147, percentage: 12, color: 'bg-purple-500' }
                ].map((item) => (
                  <div key={item.name}>
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-sm font-medium text-gray-900 dark:text-white">
                        {item.name}
                      </span>
                      <span className="text-sm text-gray-600 dark:text-gray-400">
                        ${item.revenue.toFixed(2)} ({item.percentage}%)
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
                  <span className="text-lg font-semibold text-gray-900 dark:text-white">
                    Total Revenue
                  </span>
                  <span className="text-2xl font-bold text-green-600">
                    $1,284.00
                  </span>
                </div>
              </div>
            </Card>
          </div>
        </TabsContent>

        {/* Messages Chart */}
        <TabsContent value="messages">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">
                Message Volume
              </h2>
              <Badge variant="secondary" className="capitalize">
                {granularity === 'day' ? 'Daily' : granularity === 'week' ? 'Weekly' : 'Monthly'} view
              </Badge>
            </div>
            <ResponsiveContainer width="100%" height={400}>
              <AreaChart data={messageData}>
                <defs>
                  <linearGradient id="colorMessages" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor="#2563eb" stopOpacity={0.3}/>
                    <stop offset="95%" stopColor="#2563eb" stopOpacity={0}/>
                  </linearGradient>
                </defs>
                <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                <XAxis dataKey="date" className="text-gray-600 dark:text-gray-400" />
                <YAxis className="text-gray-600 dark:text-gray-400" tickFormatter={formatNumber} />
                <Tooltip
                  contentStyle={{
                    backgroundColor: 'var(--background)',
                    border: '1px solid var(--border)',
                    borderRadius: '8px'
                  }}
                  formatter={(value?: number) => value === undefined ? ['—', 'Messages'] : [formatNumber(value), 'Messages']}
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
                <Line
                  type="monotone"
                  dataKey="sent"
                  stroke="#10b981"
                  strokeWidth={2}
                  dot={false}
                  name="Sent"
                />
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
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">
                Bandwidth Usage
              </h2>
              <Badge variant="secondary" className="capitalize">
                {granularity === 'day' ? 'Daily' : granularity === 'week' ? 'Weekly' : 'Monthly'} view
              </Badge>
            </div>
            <ResponsiveContainer width="100%" height={400}>
              <BarChart data={bandwidthData}>
                <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                <XAxis dataKey="date" className="text-gray-600 dark:text-gray-400" />
                <YAxis
                  className="text-gray-600 dark:text-gray-400"
                  tickFormatter={(value) => `${value} GB`}
                />
                <Tooltip
                  contentStyle={{
                    backgroundColor: 'var(--background)',
                    border: '1px solid var(--border)',
                    borderRadius: '8px'
                  }}
                  formatter={(value?: number) => value === undefined ? ['—', 'Bandwidth'] : [`${value} GB`, 'Bandwidth']}
                />
                <Bar dataKey="bandwidth" fill="#8b5cf6" radius={[4, 4, 0, 0]} name="Bandwidth (GB)" />
              </BarChart>
            </ResponsiveContainer>
          </Card>
        </TabsContent>

        {/* Costs Chart */}
        <TabsContent value="costs">
          <div className="grid lg:grid-cols-2 gap-6">
            <Card className="p-6">
              <div className="flex items-center justify-between mb-6">
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">
                  Cost Breakdown
                </h2>
                <Badge variant="secondary" className="capitalize">
                  {granularity === 'day' ? 'Daily' : granularity === 'week' ? 'Weekly' : 'Monthly'} view
                </Badge>
              </div>
              <div className="space-y-4">
                {costBreakdown.map((item) => (
                  <div key={item.name}>
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-sm font-medium text-gray-900 dark:text-white">
                        {item.name}
                      </span>
                      <span className="text-sm text-gray-600 dark:text-gray-400">
                        ${(item.cost / 100).toFixed(2)} ({item.percentage}%)
                      </span>
                    </div>
                    <div className="h-3 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                      <div
                        className="h-full bg-blue-600 rounded-full transition-all"
                        style={{ width: `${item.percentage}%` }}
                      />
                    </div>
                  </div>
                ))}
              </div>
              <div className="mt-6 pt-6 border-t border-gray-200 dark:border-gray-700">
                <div className="flex items-center justify-between">
                  <span className="text-lg font-semibold text-gray-900 dark:text-white">
                    Total This Month
                  </span>
                  <span className="text-2xl font-bold text-gray-900 dark:text-white">
                    $45.23
                  </span>
                </div>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                  Projected: $52.00 based on current usage
                </p>
              </div>
            </Card>

            <Card className="p-6">
              <div className="flex items-center justify-between mb-6">
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">
                  Cost Trend
                </h2>
                <Badge variant="secondary" className="capitalize">
                  {granularity === 'day' ? 'Daily' : granularity === 'week' ? 'Weekly' : 'Monthly'} view
                </Badge>
              </div>
              <ResponsiveContainer width="100%" height={300}>
                <LineChart data={messageData}>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                  <XAxis dataKey="date" className="text-gray-600 dark:text-gray-400" />
                  <YAxis
                    className="text-gray-600 dark:text-gray-400"
                    tickFormatter={(value) => `$${value / 100}`}
                  />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: 'var(--background)',
                      border: '1px solid var(--border)',
                      borderRadius: '8px'
                    }}
                    formatter={(value?: number) => value === undefined ? ['—', 'Cost'] : [`$${(value / 100).toFixed(2)}`, 'Cost']}
                  />
                  <Line
                    type="monotone"
                    dataKey="messages"
                    stroke="#2563eb"
                    strokeWidth={2}
                    dot={false}
                    name="Cost"
                  />
                </LineChart>
              </ResponsiveContainer>
            </Card>
          </div>
        </TabsContent>

        {/* Hourly Chart */}
        <TabsContent value="hourly">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">
                Hourly Message Distribution
              </h2>
              <Badge variant="secondary">Average over selected period</Badge>
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
                    borderRadius: '8px'
                  }}
                  formatter={(value?: number) => value === undefined ? ['—', 'Messages'] : [value, 'Messages']}
                  labelFormatter={(value) => `${value}:00`}
                />
                <Bar dataKey="messages" fill="#10b981" radius={[4, 4, 0, 0]} name="Messages" />
              </BarChart>
            </ResponsiveContainer>
          </Card>
        </TabsContent>
      </Tabs>

      {/* Quick Insights */}
      <div className="mt-8 grid lg:grid-cols-3 gap-6">
        <Card className="p-6">
          <h3 className="text-lg font-semibold mb-4 text-gray-900 dark:text-white">
            Peak Hours
          </h3>
          <p className="text-3xl font-bold text-gray-900 dark:text-white mb-2">
            2 PM - 4 PM
          </p>
          <p className="text-sm text-gray-600 dark:text-gray-400">
            Highest message volume occurs during afternoon hours
          </p>
        </Card>

        <Card className="p-6">
          <h3 className="text-lg font-semibold mb-4 text-gray-900 dark:text-white">
            Best Day
          </h3>
          <p className="text-3xl font-bold text-gray-900 dark:text-white mb-2">
            Thursday
          </p>
          <p className="text-sm text-gray-600 dark:text-gray-400">
            15% more messages compared to average
          </p>
        </Card>

        <Card className="p-6">
          <h3 className="text-lg font-semibold mb-4 text-gray-900 dark:text-white">
            Growth Trend
          </h3>
          <p className="text-3xl font-bold text-green-600 mb-2">
            +23.5%
          </p>
          <p className="text-sm text-gray-600 dark:text-gray-400">
            Month-over-month message growth
          </p>
        </Card>
      </div>
    </DashboardLayout>
  );
}
