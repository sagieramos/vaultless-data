"use client";
import Link from 'next/link';
import { motion } from 'motion/react';
import { Plus, BarChart3, Key, Book, TrendingUp, Server, Users, CreditCard, MessageSquare, AppWindow, Zap, ChevronDown } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import DashboardLayout from '../components/layout/DashboardLayout';
import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
import { useRequireAuth } from '@/contexts/AuthContext';
import { useEffect, useState } from 'react';
import { analyticsApi, applicationsApi } from '@/lib/api';

const recentActivity = [
  { type: 'message', text: 'Message sent to user_abc123', time: '2 minutes ago' },
  { type: 'app', text: 'Application "Production API" created', time: '1 hour ago' },
  { type: 'key', text: 'API key rotated', time: '3 hours ago' },
  { type: 'message', text: 'Message sent to user_xyz789', time: '5 hours ago' },
];

const quickActions = [
  { icon: Plus, title: 'New App', description: 'Create a new application', link: '/applications/new', color: 'bg-blue-500' },
  { icon: BarChart3, title: 'View Analytics', description: 'See your usage stats', link: '/analytics', color: 'bg-purple-500' },
  { icon: Key, title: 'Manage Keys', description: 'Rotate and manage API keys', link: '/api-keys', color: 'bg-green-500' },
  { icon: Book, title: 'Documentation', description: 'Browse our guides', link: '/docs', color: 'bg-orange-500' },
];

interface DashboardStats {
  totalRevenue: number;
  revenueChange: number;
  activeSubscribers: number;
  subscribersChange: number;
  activeApps: number;
}

export default function DashboardPage() {
  const { user } = useRequireAuth();
  const [usageData, setUsageData] = useState<{ name: string; messagesSent: number; proofsVerified: number; dataTransferGB: number }[]>([]);
  const [dateRange, setDateRange] = useState<'daily' | 'monthly'>('monthly');
  const [displayMetrics, setDisplayMetrics] = useState<{ messages: boolean; proofs: boolean; data: boolean }>({
    messages: true,
    proofs: false,
    data: false
  });
  const [stats, setStats] = useState<DashboardStats | null>(null);
  const [isLoadingStats, setIsLoadingStats] = useState(true);
  const [isLoadingChart, setIsLoadingChart] = useState(true);

  const toggleMetric = (metric: 'messages' | 'proofs' | 'data') => {
    setDisplayMetrics(prev => ({ ...prev, [metric]: !prev[metric] }));
  };

  const hasApps = stats ? stats.activeApps > 0 : true;

  const userDisplayName = user?.name || user?.email?.split('@')[0] || 'User';

  // Fetch dashboard stats (apps, revenue, subscribers)
  useEffect(() => {
    const fetchStats = async () => {
      setIsLoadingStats(true);
      try {
        const appsResponse = await applicationsApi.list({ page: 1, pageSize: 100 });
        const activeApps = appsResponse.data.filter(app => app.isActive).length;

        // Calculate total revenue and subscribers from apps data
        let totalRevenue = 0;
        let totalSubscribers = 0;
        appsResponse.data.forEach(app => {
          // Estimate revenue based on usage (this would ideally come from a dedicated endpoint)
          totalSubscribers += app.clientCount || 0;
        });

        // For now, use estimated values - in production these would come from a billing API
        totalRevenue = totalSubscribers * 9; // Rough estimate per subscriber

        setStats({
          totalRevenue,
          revenueChange: 24, // Would come from comparing with previous period
          activeSubscribers: totalSubscribers,
          subscribersChange: 12, // Would come from comparing with previous period
          activeApps,
        });
      } catch (error) {
        console.error('Error fetching stats:', error);
        // Fallback to mock data
        setStats({
          totalRevenue: 1284,
          revenueChange: 24,
          activeSubscribers: 142,
          subscribersChange: 12,
          activeApps: 4,
        });
      } finally {
        setIsLoadingStats(false);
      }
    };

    fetchStats();
  }, []);

  // Fetch usage chart data
  useEffect(() => {
    const fetchUsageData = async () => {
      setIsLoadingChart(true);
      try {
        const period = dateRange === 'daily' ? '14d' : '12m';
        const response = await analyticsApi.getUsageOverTime(period);

        const processedData = response.data.map((point) => {
          const date = new Date(point.date);
          const displayLabel = dateRange === 'daily'
            ? date.toLocaleDateString('en-US', { month: 'numeric', day: 'numeric' })
            : date.toLocaleDateString('en-US', { month: 'short' });

          return {
            name: displayLabel,
            messagesSent: point.messages_sent,
            proofsVerified: point.proofs_verified,
            dataTransferGB: (point as any).data_transfer_gb ?? 0
          };
        });

        setUsageData(processedData);
      } catch (error) {
        console.error('Error fetching usage data:', error);
        // Fallback to mock data if API call fails
        const mockData = dateRange === 'daily'
          ? [
              { name: '1/13', messagesSent: 1240, proofsVerified: 890, dataTransferGB: 2.4 },
              { name: '1/14', messagesSent: 1580, proofsVerified: 1120, dataTransferGB: 3.1 },
              { name: '1/15', messagesSent: 1320, proofsVerified: 980, dataTransferGB: 2.6 },
              { name: '1/16', messagesSent: 1890, proofsVerified: 1340, dataTransferGB: 3.8 },
              { name: '1/17', messagesSent: 2100, proofsVerified: 1560, dataTransferGB: 4.2 },
              { name: '1/18', messagesSent: 1750, proofsVerified: 1280, dataTransferGB: 3.5 },
              { name: '1/19', messagesSent: 980, proofsVerified: 720, dataTransferGB: 1.9 },
              { name: '1/20', messagesSent: 1450, proofsVerified: 1050, dataTransferGB: 2.9 },
              { name: '1/21', messagesSent: 1680, proofsVerified: 1220, dataTransferGB: 3.3 },
              { name: '1/22', messagesSent: 2250, proofsVerified: 1680, dataTransferGB: 4.5 },
              { name: '1/23', messagesSent: 2480, proofsVerified: 1840, dataTransferGB: 4.9 },
              { name: '1/24', messagesSent: 2120, proofsVerified: 1580, dataTransferGB: 4.2 },
              { name: '1/25', messagesSent: 1340, proofsVerified: 980, dataTransferGB: 2.7 },
              { name: '1/26', messagesSent: 1890, proofsVerified: 1420, dataTransferGB: 3.8 },
            ]
          : [
              { name: 'Feb', messagesSent: 28500, proofsVerified: 21200, dataTransferGB: 57.2 },
              { name: 'Mar', messagesSent: 32400, proofsVerified: 24800, dataTransferGB: 64.8 },
              { name: 'Apr', messagesSent: 38200, proofsVerified: 29100, dataTransferGB: 76.4 },
              { name: 'May', messagesSent: 41500, proofsVerified: 31800, dataTransferGB: 83.0 },
              { name: 'Jun', messagesSent: 45800, proofsVerified: 35200, dataTransferGB: 91.6 },
              { name: 'Jul', messagesSent: 52100, proofsVerified: 40500, dataTransferGB: 104.2 },
              { name: 'Aug', messagesSent: 48900, proofsVerified: 37800, dataTransferGB: 97.8 },
              { name: 'Sep', messagesSent: 55200, proofsVerified: 42600, dataTransferGB: 110.4 },
              { name: 'Oct', messagesSent: 61800, proofsVerified: 48200, dataTransferGB: 123.6 },
              { name: 'Nov', messagesSent: 58400, proofsVerified: 45100, dataTransferGB: 116.8 },
              { name: 'Dec', messagesSent: 64200, proofsVerified: 50800, dataTransferGB: 128.4 },
              { name: 'Jan', messagesSent: 71500, proofsVerified: 56200, dataTransferGB: 143.0 },
            ];

        setUsageData(mockData);
      } finally {
        setIsLoadingChart(false);
      }
    };

    fetchUsageData();
  }, [dateRange]);

  return (
    <DashboardLayout>
      {/* Welcome Banner */}
      <motion.div
        initial={{ opacity: 0, y: -20 }}
        animate={{ opacity: 1, y: 0 }}
        className="mb-6 sm:mb-8"
      >
        <div>
          <h1 className="text-2xl sm:text-3xl font-bold text-gray-900 dark:text-white">
            Welcome back, {userDisplayName}!
          </h1>
          <p className="text-sm sm:text-base text-gray-600 dark:text-gray-400 mt-1">
            {hasApps ? `You have ${stats?.activeApps || 0} active application${stats?.activeApps !== 1 ? 's' : ''}` : 'Ready to get started?'}
          </p>
        </div>
      </motion.div>

      {hasApps ? (
        <>
          {/* Stats Row */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mb-8">
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.1 }}
            >
              <Card className="p-6 h-full">
                <div className="flex items-center gap-4">
                  <div className="w-12 h-12 rounded-xl bg-teal-100 dark:bg-teal-900/30 flex items-center justify-center flex-shrink-0">
                    <CreditCard className="w-6 h-6 text-teal-600" />
                  </div>
                  <div className="flex-grow">
                    <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Total Revenue</span>
                    <div className="flex items-baseline gap-3 mt-1">
                      {isLoadingStats ? (
                        <div className="h-9 w-24 bg-gray-200 dark:bg-gray-700 rounded animate-pulse" />
                      ) : (
                        <>
                          <span className="text-3xl font-bold text-gray-900 dark:text-white">
                            ${stats?.totalRevenue.toLocaleString() || '0'}
                          </span>
                          <span className="text-sm font-medium text-green-600 flex items-center">
                            <TrendingUp className="w-4 h-4 mr-1" />
                            +{stats?.revenueChange || 0}%
                          </span>
                        </>
                      )}
                    </div>
                  </div>
                  <div className="text-right flex-shrink-0 hidden sm:block">
                    <span className="text-xs text-gray-500 dark:text-gray-400">This month</span>
                  </div>
                </div>
              </Card>
            </motion.div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.2 }}
            >
              <Card className="p-6 h-full">
                <div className="flex items-center gap-4">
                  <div className="w-12 h-12 rounded-xl bg-orange-100 dark:bg-orange-900/30 flex items-center justify-center flex-shrink-0">
                    <Users className="w-6 h-6 text-orange-600" />
                  </div>
                  <div className="flex-grow">
                    <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Active Subscribers</span>
                    <div className="flex items-baseline gap-3 mt-1">
                      {isLoadingStats ? (
                        <div className="h-9 w-16 bg-gray-200 dark:bg-gray-700 rounded animate-pulse" />
                      ) : (
                        <>
                          <span className="text-3xl font-bold text-gray-900 dark:text-white">
                            {stats?.activeSubscribers.toLocaleString() || '0'}
                          </span>
                          <span className="text-sm font-medium text-green-600 flex items-center">
                            <TrendingUp className="w-4 h-4 mr-1" />
                            +{stats?.subscribersChange || 0}%
                          </span>
                        </>
                      )}
                    </div>
                  </div>
                  <div className="text-right flex-shrink-0 hidden sm:block">
                    <span className="text-xs text-gray-500 dark:text-gray-400">Across all apps</span>
                  </div>
                </div>
              </Card>
            </motion.div>
          </div>


          {/* Charts and Activity Row */}
          <div className="grid lg:grid-cols-3 gap-6 mb-8">
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.2 }}
              className="lg:col-span-2"
            >
              <Card className="p-4 sm:p-6 h-full flex flex-col">
                <div className="flex flex-col gap-4 mb-6">
                  <div className="flex justify-between items-center">
                    <h2 className="text-lg sm:text-xl font-semibold text-gray-900 dark:text-white">Usage Overview</h2>
                    <div className="relative">
                      <select
                        value={dateRange}
                        onChange={(e) => setDateRange(e.target.value as 'daily' | 'monthly')}
                        aria-label="Select date range"
                        className="bg-gray-100 dark:bg-gray-800 border-0 rounded-lg py-1.5 sm:py-2 pl-3 pr-8 text-sm font-medium text-gray-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-blue-500 appearance-none cursor-pointer"
                      >
                        <option value="daily">Daily</option>
                        <option value="monthly">Monthly</option>
                      </select>
                      <ChevronDown className="absolute right-2 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-500 dark:text-gray-400 pointer-events-none" />
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-2 sm:gap-3">
                    <button
                      onClick={() => toggleMetric('messages')}
                      className={`flex items-center gap-1.5 px-2 sm:px-3 py-1 sm:py-1.5 text-xs sm:text-sm font-medium rounded-lg transition-colors ${
                        displayMetrics.messages
                          ? 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300'
                          : 'bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                      }`}
                    >
                      <span className={`w-1.5 h-1.5 sm:w-2 sm:h-2 rounded-full ${displayMetrics.messages ? 'bg-blue-500' : 'bg-gray-400'}`} />
                      Messages
                    </button>
                    <button
                      onClick={() => toggleMetric('proofs')}
                      className={`flex items-center gap-1.5 px-2 sm:px-3 py-1 sm:py-1.5 text-xs sm:text-sm font-medium rounded-lg transition-colors ${
                        displayMetrics.proofs
                          ? 'bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-300'
                          : 'bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                      }`}
                    >
                      <span className={`w-1.5 h-1.5 sm:w-2 sm:h-2 rounded-full ${displayMetrics.proofs ? 'bg-emerald-500' : 'bg-gray-400'}`} />
                      Proofs
                    </button>
                    <button
                      onClick={() => toggleMetric('data')}
                      className={`flex items-center gap-1.5 px-2 sm:px-3 py-1 sm:py-1.5 text-xs sm:text-sm font-medium rounded-lg transition-colors ${
                        displayMetrics.data
                          ? 'bg-violet-100 dark:bg-violet-900/30 text-violet-700 dark:text-violet-300'
                          : 'bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                      }`}
                    >
                      <span className={`w-1.5 h-1.5 sm:w-2 sm:h-2 rounded-full ${displayMetrics.data ? 'bg-violet-500' : 'bg-gray-400'}`} />
                      Data
                    </button>
                  </div>
                </div>
                <div className="flex-grow min-h-[250px] sm:min-h-[350px]">
                  {isLoadingChart ? (
                    <div className="w-full h-full flex items-center justify-center">
                      <div className="w-full h-full bg-gray-100 dark:bg-gray-800 rounded-lg animate-pulse" />
                    </div>
                  ) : (
                  <ResponsiveContainer width="100%" height="100%">
                    <AreaChart data={usageData} margin={{ top: 10, right: 10, left: 0, bottom: 0 }}>
                      <defs>
                        <linearGradient id="colorMessages" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="0%" stopColor="#3b82f6" stopOpacity={0.4}/>
                          <stop offset="100%" stopColor="#3b82f6" stopOpacity={0.05}/>
                        </linearGradient>
                        <linearGradient id="colorProofs" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="0%" stopColor="#10b981" stopOpacity={0.4}/>
                          <stop offset="100%" stopColor="#10b981" stopOpacity={0.05}/>
                        </linearGradient>
                        <linearGradient id="colorData" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="0%" stopColor="#8b5cf6" stopOpacity={0.4}/>
                          <stop offset="100%" stopColor="#8b5cf6" stopOpacity={0.05}/>
                        </linearGradient>
                      </defs>
                      <CartesianGrid
                        strokeDasharray="3 3"
                        vertical={false}
                        stroke="#e5e7eb"
                        className="dark:stroke-gray-700/50"
                      />
                      <XAxis
                        dataKey="name"
                        axisLine={false}
                        tickLine={false}
                        tick={{ fontSize: 11, fill: '#9ca3af' }}
                        dy={10}
                        interval="preserveStartEnd"
                        minTickGap={30}
                      />
                      <YAxis
                        yAxisId="count"
                        axisLine={false}
                        tickLine={false}
                        tick={{ fontSize: 12, fill: '#9ca3af' }}
                        domain={[0, 'auto']}
                        tickFormatter={(value) => {
                          return value >= 1000 ? (value / 1000).toFixed(1) + 'K' : value.toString();
                        }}
                        width={45}
                        hide={!displayMetrics.messages && !displayMetrics.proofs}
                      />
                      <YAxis
                        yAxisId="data"
                        orientation="right"
                        axisLine={false}
                        tickLine={false}
                        tick={{ fontSize: 12, fill: '#9ca3af' }}
                        domain={[0, 'auto']}
                        tickFormatter={(value) => value.toFixed(1) + ' GB'}
                        width={55}
                        hide={!displayMetrics.data}
                      />
                      <Tooltip
                        content={({ active, payload, label }) => {
                          if (!active || !payload?.length) return null;
                          return (
                            <div className="bg-white/95 dark:bg-gray-800/95 border-0 rounded-xl shadow-lg p-3 sm:p-4">
                              <p className="text-sm font-medium text-gray-900 dark:text-white mb-2">{label}</p>
                              {payload.map((entry, index) => {
                                const value = Number(entry.value) || 0;
                                let displayValue = value.toLocaleString();
                                let displayName = entry.name;
                                if (entry.dataKey === 'dataTransferGB') {
                                  displayValue = value.toFixed(2) + ' GB';
                                  displayName = 'Data Transfer';
                                } else if (entry.dataKey === 'messagesSent') {
                                  displayName = 'Messages Sent';
                                } else if (entry.dataKey === 'proofsVerified') {
                                  displayName = 'Proofs Verified';
                                }
                                return (
                                  <div key={index} className="flex items-center gap-2 text-sm">
                                    <span className="w-2 h-2 rounded-full" style={{ backgroundColor: entry.color }} />
                                    <span className="text-gray-600 dark:text-gray-400">{displayName}:</span>
                                    <span className="font-medium text-gray-900 dark:text-white">{displayValue}</span>
                                  </div>
                                );
                              })}
                            </div>
                          );
                        }}
                        cursor={{ stroke: '#d1d5db', strokeWidth: 1, strokeDasharray: '4 4' }}
                      />
                      {displayMetrics.messages && (
                        <Area
                          yAxisId="count"
                          type="natural"
                          dataKey="messagesSent"
                          stroke="#3b82f6"
                          strokeWidth={2.5}
                          fillOpacity={1}
                          fill="url(#colorMessages)"
                          name="Messages Sent"
                          dot={false}
                          activeDot={{ r: 6, fill: '#3b82f6', stroke: '#fff', strokeWidth: 2 }}
                        />
                      )}
                      {displayMetrics.proofs && (
                        <Area
                          yAxisId="count"
                          type="natural"
                          dataKey="proofsVerified"
                          stroke="#10b981"
                          strokeWidth={2.5}
                          fillOpacity={1}
                          fill="url(#colorProofs)"
                          name="Proofs Verified"
                          dot={false}
                          activeDot={{ r: 6, fill: '#10b981', stroke: '#fff', strokeWidth: 2 }}
                        />
                      )}
                      {displayMetrics.data && (
                        <Area
                          yAxisId="data"
                          type="natural"
                          dataKey="dataTransferGB"
                          stroke="#8b5cf6"
                          strokeWidth={2.5}
                          fillOpacity={1}
                          fill="url(#colorData)"
                          name="Data Transfer"
                          dot={false}
                          activeDot={{ r: 6, fill: '#8b5cf6', stroke: '#fff', strokeWidth: 2 }}
                        />
                      )}
                    </AreaChart>
                  </ResponsiveContainer>
                  )}
                </div>
              </Card>
            </motion.div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.3 }}
            >
              <Card className="p-4 sm:p-6 h-full flex flex-col">
                <h2 className="text-lg sm:text-xl font-semibold mb-4 sm:mb-6 text-gray-900 dark:text-white">Recent Activity</h2>
                <div className="space-y-3 sm:space-y-4 flex-grow">
                  {recentActivity.map((activity) => {
                    const iconConfig = {
                      message: { icon: MessageSquare, bg: 'bg-blue-100 dark:bg-blue-900/30', color: 'text-blue-600 dark:text-blue-400' },
                      app: { icon: AppWindow, bg: 'bg-green-100 dark:bg-green-900/30', color: 'text-green-600 dark:text-green-400' },
                      key: { icon: Key, bg: 'bg-orange-100 dark:bg-orange-900/30', color: 'text-orange-600 dark:text-orange-400' },
                    }[activity.type] || { icon: Zap, bg: 'bg-gray-100 dark:bg-gray-800', color: 'text-gray-600 dark:text-gray-400' };
                    const IconComponent = iconConfig.icon;

                    return (
                      <div key={activity.text} className="flex items-start gap-3">
                        <div className={`w-8 h-8 rounded-lg ${iconConfig.bg} flex items-center justify-center flex-shrink-0`}>
                          <IconComponent className={`w-4 h-4 ${iconConfig.color}`} />
                        </div>
                        <div className="flex-1 min-w-0">
                          <p className="text-sm text-gray-900 dark:text-white truncate">{activity.text}</p>
                          <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">{activity.time}</p>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </Card>
            </motion.div>
          </div>

          {/* Quick Actions */}
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.4 }}
          >
            <h2 className="text-lg sm:text-xl font-semibold mb-4 sm:mb-6 text-gray-900 dark:text-white">Quick Actions</h2>
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-3 sm:gap-6">
              {quickActions.map((action) => (
                <Link key={action.title} href={action.link}>
                  <Card className="p-3 sm:p-6 h-full hover:shadow-lg transition-all cursor-pointer group flex flex-col">
                    <div className={`w-10 h-10 sm:w-12 sm:h-12 rounded-lg ${action.color} flex items-center justify-center mb-2 sm:mb-4 group-hover:scale-110 transition-transform`}>
                      <action.icon className="w-5 h-5 sm:w-6 sm:h-6 text-white" />
                    </div>
                    <h3 className="text-sm sm:text-base font-semibold text-gray-900 dark:text-white mb-0.5 sm:mb-1">{action.title}</h3>
                    <p className="text-xs sm:text-sm text-gray-600 dark:text-gray-400 flex-grow hidden sm:block">{action.description}</p>
                  </Card>
                </Link>
              ))}
            </div>
          </motion.div>
        </>
      ) : (
        /* Empty State */
        <motion.div
          initial={{ opacity: 0, scale: 0.95 }}
          animate={{ opacity: 1, scale: 1 }}
          className="flex items-center justify-center py-10 sm:py-20 px-4"
        >
          <Card className="p-6 sm:p-12 text-center max-w-md w-full">
            <div className="w-16 h-16 sm:w-20 sm:h-20 bg-blue-100 dark:bg-blue-900/20 rounded-full flex items-center justify-center mx-auto mb-4 sm:mb-6">
              <Server className="w-8 h-8 sm:w-10 sm:h-10 text-blue-600" />
            </div>
            <h2 className="text-xl sm:text-2xl font-bold mb-2 text-gray-900 dark:text-white">
              Ready to build something great?
            </h2>
            <p className="text-sm sm:text-base text-gray-600 dark:text-gray-400 mb-4 sm:mb-6">
              Create your first application to start sending secure messages
            </p>
            <Link href="/applications/new">
              <Button size="lg" className="bg-blue-600 hover:bg-blue-700 w-full sm:w-auto">
                <Plus className="w-5 h-5 mr-2" />
                <span className="hidden sm:inline">Create Your First Application</span>
                <span className="sm:hidden">Create Application</span>
              </Button>
            </Link>
            <p className="text-xs sm:text-sm text-gray-600 dark:text-gray-400 mt-3 sm:mt-4">
              No coding required • 2 minutes to setup
            </p>
          </Card>
        </motion.div>
      )}
    </DashboardLayout>
  );
}
