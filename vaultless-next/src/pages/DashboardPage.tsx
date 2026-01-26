"use client";
import Link from 'next/link';
import { motion } from 'motion/react';
import { Plus, BarChart3, Key, Book, TrendingUp, MessageSquare, Server, Zap, Users, CreditCard, DollarSign } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Popover, PopoverTrigger, PopoverContent } from '../components/ui/popover';
import DashboardLayout from '../components/layout/DashboardLayout';
import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
import { useRequireAuth } from '@/contexts/AuthContext';
import { useEffect, useState } from 'react';
import { analyticsApi } from '@/lib/api';
import type { UsageOverTime } from '@/types/api';

const data = [
  { name: 'Mon', messages: 4000 },
  { name: 'Tue', messages: 3000 },
  { name: 'Wed', messages: 5000 },
  { name: 'Thu', messages: 2780 },
  { name: 'Fri', messages: 1890 },
  { name: 'Sat', messages: 2390 },
  { name: 'Sun', messages: 3490 },
];

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

export default function DashboardPage() {
  const { user } = useRequireAuth();
  const [bandwidthData, setBandwidthData] = useState<{ name: string; bytes: number; messages: number }[]>([]);
  const [totalBandwidth, setTotalBandwidth] = useState<number>(0);
  const [bandwidthGrowth, setBandwidthGrowth] = useState<number>(0);
  const [dateRange, setDateRange] = useState<'daily' | 'monthly'>('monthly');
  const [displayDataTypes, setDisplayDataTypes] = useState<{ messages: boolean; dataTransfer: boolean }>({
    messages: true,
    dataTransfer: true
  });

  const hasApps = true; // Change to false to see empty state

  const userDisplayName = user?.name || user?.email?.split('@')[0] || 'User';

  useEffect(() => {
    // Fetch bandwidth usage data
    const fetchBandwidthData = async () => {
      try {
        // Determine the period based on the selected date range
        const period = dateRange === 'daily' ? '7d' : '30d';
        const response = await analyticsApi.getUsageOverTime(period);

        // Process the data to get both messages and bandwidth info
        // Using bytesSent and bytesReceived from the UsageStats if available
        // For now, we'll simulate the data based on messages since the API response structure isn't fully clear
        const processedData = response.data.map((point, index) => {
          // Generate random bytes and messages to avoid overlap for development
          const simulatedBytes = Math.round(Math.random() * 1000000000 + 500000000); // Random bytes between 500MB and 1.5GB
          const simulatedMessages = Math.round(Math.random() * 500 + 100); // Random messages between 100 and 600

          // Extract day from date for display based on the selected range
          const date = new Date(point.date);
          const displayLabel = dateRange === 'daily'
            ? date.toLocaleDateString('en-US', { month: 'numeric', day: 'numeric' }) // Show date in d/m format for daily view
            : date.toLocaleDateString('en-US', { month: 'numeric', year: '2-digit' }); // Show date in m/yy format for monthly view

          return {
            name: displayLabel,
            bytes: simulatedBytes,
            messages: simulatedMessages
          };
        });

        setBandwidthData(processedData);

        // Calculate total bandwidth
        const total = processedData.reduce((sum, point) => sum + point.bytes, 0);
        setTotalBandwidth(total);

        // Calculate growth percentage
        if (processedData.length >= 2) {
          const recent = processedData[processedData.length - 1].bytes;
          const previous = processedData[processedData.length - 2].bytes;
          const growth = previous !== 0 ? ((recent - previous) / previous) * 100 : 0;
          setBandwidthGrowth(parseFloat(growth.toFixed(1)));
        }
      } catch (error) {
        console.error('Error fetching bandwidth data:', error);
        // Fallback to mock data if API call fails
        const mockData = dateRange === 'daily'
          ? [
              { name: '1/15', bytes: 800000000, messages: 450 }, // 800MB, 450 messages
              { name: '1/16', bytes: 1200000000, messages: 200 }, // 1.2GB, 200 messages
              { name: '1/17', bytes: 600000000, messages: 550 }, // 600MB, 550 messages
              { name: '1/18', bytes: 1500000000, messages: 150 }, // 1.5GB, 150 messages
              { name: '1/19', bytes: 900000000, messages: 350 }, // 900MB, 350 messages
              { name: '1/20', bytes: 1100000000, messages: 250 }, // 1.1GB, 250 messages
              { name: '1/21', bytes: 700000000, messages: 400 }, // 700MB, 400 messages
            ]
          : [
              { name: '12/24', bytes: 2500000000, messages: 120 }, // 2.5GB, 120 messages
              { name: '1/25', bytes: 1800000000, messages: 380 }, // 1.8GB, 380 messages
              { name: '2/25', bytes: 3200000000, messages: 80 }, // 3.2GB, 80 messages
              { name: '3/25', bytes: 1500000000, messages: 420 }, // 1.5GB, 420 messages
              { name: '4/25', bytes: 2800000000, messages: 180 }, // 2.8GB, 180 messages
              { name: '5/25', bytes: 2100000000, messages: 320 }, // 2.1GB, 320 messages
              { name: '6/25', bytes: 3500000000, messages: 90 }, // 3.5GB, 90 messages
            ];

        setBandwidthData(mockData);
        const total = mockData.reduce((sum, point) => sum + point.bytes, 0);
        setTotalBandwidth(total);
        setBandwidthGrowth(8.0);
      }
    };

    fetchBandwidthData();
  }, [dateRange]);

  // Format bytes to human readable format (GB, MB, etc.)
  const formatBytes = (bytes: number): string => {
    if (bytes === 0) return '0 Bytes';

    const k = 1024;
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));

    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  };

  return (
    <DashboardLayout>
      {/* Welcome Banner */}
      <motion.div
        initial={{ opacity: 0, y: -20 }}
        animate={{ opacity: 1, y: 0 }}
        className="mb-8"
      >
        <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
          <div>
            <h1 className="text-3xl font-bold text-gray-900 dark:text-white flex items-center gap-2">
              Welcome back, {userDisplayName}!
              <Popover>
                <PopoverTrigger asChild>
                  <Button variant="ghost" size="icon" className="rounded-full h-8 w-8">
                    <Zap className="h-4 w-4 text-yellow-500" />
                  </Button>
                </PopoverTrigger>
                <PopoverContent className="w-64">
                  <div className="space-y-2">
                    <h4 className="font-medium leading-none">Status: Active</h4>
                    <p className="text-sm text-gray-500">
                      Your current plan is Pro. You have 35k messages remaining this month.
                    </p>
                  </div>
                </PopoverContent>
              </Popover>
            </h1>
            <p className="text-gray-600 dark:text-gray-400 mt-1">
              {hasApps ? 'You have 4 active applications' : 'Ready to get started?'}
            </p>
          </div>
          <Link href="/applications/new">
            <Button className="bg-blue-600 hover:bg-blue-700">
              <Plus className="w-4 h-4 mr-2" />
              New Application
            </Button>
          </Link>
        </div>
      </motion.div>

      {hasApps ? (
        <>
          {/* Stats Row */}
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6 mb-8">
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.1 }}
            >
              <Card className="p-6 h-full flex flex-col border-teal-200 dark:border-teal-800 bg-teal-50 dark:bg-teal-900/10">
                <div className="flex items-center justify-between mb-4">
                  <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Total Revenue</span>
                  <CreditCard className="w-5 h-5 text-teal-600" />
                </div>
                <div className="flex items-end gap-2 flex-grow">
                  <span className="text-3xl font-bold text-gray-900 dark:text-white">$1,284</span>
                  <span className="text-sm text-green-600 flex items-center">
                    <TrendingUp className="w-4 h-4 mr-1" />
                    +24%
                  </span>
                </div>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-4">This month</p>
              </Card>
            </motion.div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.2 }}
            >
              <Card className="p-6 h-full flex flex-col border-orange-200 dark:border-orange-800 bg-orange-50 dark:bg-orange-900/10">
                <div className="flex items-center justify-between mb-4">
                  <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Active Subscribers</span>
                  <Users className="w-5 h-5 text-orange-600" />
                </div>
                <div className="flex items-end gap-2 flex-grow">
                  <span className="text-3xl font-bold text-gray-900 dark:text-white">142</span>
                  <span className="text-sm text-green-600 flex items-center">
                    <TrendingUp className="w-4 h-4 mr-1" />
                    +12%
                  </span>
                </div>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-4">Across all apps</p>
              </Card>
            </motion.div>

            {/* Empty placeholders to maintain grid alignment */}
            <div className="hidden md:block"></div>
            <div className="hidden md:block"></div>
          </div>


          {/* Charts and Activity Row */}
          <div className="grid lg:grid-cols-3 gap-6 mb-8">
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.2 }}
              className="lg:col-span-2"
            >
              <Card className="p-6 h-full flex flex-col">
                <div className="flex justify-between items-center mb-6">
                  <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Usage Overview</h2>
                  <div className="flex gap-3">
                    <div className="flex items-center">
                      <input
                        type="checkbox"
                        id="messages-toggle"
                        checked={displayDataTypes.messages}
                        onChange={() => setDisplayDataTypes(prev => ({ ...prev, messages: !prev.messages }))}
                        className="mr-2 h-4 w-4 text-blue-600 rounded focus:ring-blue-500"
                      />
                      <label htmlFor="messages-toggle" className="text-sm text-gray-700 dark:text-gray-300">Messages</label>
                    </div>
                    <div className="flex items-center">
                      <input
                        type="checkbox"
                        id="data-transfer-toggle"
                        checked={displayDataTypes.dataTransfer}
                        onChange={() => setDisplayDataTypes(prev => ({ ...prev, dataTransfer: !prev.dataTransfer }))}
                        className="mr-2 h-4 w-4 text-blue-600 rounded focus:ring-blue-500"
                      />
                      <label htmlFor="data-transfer-toggle" className="text-sm text-gray-700 dark:text-gray-300">Data Transfer</label>
                    </div>
                    <div className="relative">
                      <select
                        value={dateRange}
                        onChange={(e) => setDateRange(e.target.value as 'daily' | 'monthly')}
                        className="bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-700 rounded-lg py-2 pl-3 pr-8 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500 appearance-none"
                      >
                        <option value="daily">Daily</option>
                        <option value="monthly">Monthly</option>
                      </select>
                      <div className="pointer-events-none absolute inset-y-0 right-0 flex items-center px-2 text-gray-700 dark:text-gray-300">
                        <svg className="fill-current h-4 w-4" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20">
                          <path d="M9.293 12.95l.707.707L15.657 8l-1.414-1.414L10 10.828 5.757 6.586 4.343 8z" />
                        </svg>
                      </div>
                    </div>
                  </div>
                </div>
                <div className="flex-grow">
                  <ResponsiveContainer width="100%" height={350}>
                    <AreaChart data={bandwidthData}>
                      <defs>
                        {displayDataTypes.messages && (
                          <linearGradient id="colorMessages" x1="0" y1="0" x2="0" y2="1">
                            <stop offset="5%" stopColor="#10b981" stopOpacity={0.3}/>
                            <stop offset="95%" stopColor="#10b981" stopOpacity={0}/>
                          </linearGradient>
                        )}
                        {displayDataTypes.dataTransfer && (
                          <linearGradient id="colorDataTransfer" x1="0" y1="0" x2="0" y2="1">
                            <stop offset="5%" stopColor="#3b82f6" stopOpacity={0.3}/>
                            <stop offset="95%" stopColor="#3b82f6" stopOpacity={0}/>
                          </linearGradient>
                        )}
                      </defs>
                      <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                      <XAxis dataKey="name" className="text-gray-600 dark:text-gray-400" />
                      <YAxis
                        yAxisId="left"
                        orientation="left"
                        className="text-gray-600 dark:text-gray-400"
                        domain={[0, 'auto']}
                        tickFormatter={(value) => {
                          // Convert bytes to GB for display
                          const gb = value / (1024 * 1024 * 1024);
                          return gb.toFixed(1) + 'GB';
                        }}
                      />
                      <YAxis
                        yAxisId="right"
                        orientation="right"
                        className="text-gray-600 dark:text-gray-400"
                        domain={[0, 'auto']}
                        tickFormatter={(value) => {
                          // Format messages with K for thousands
                          return (value / 1000).toFixed(1) + 'K';
                        }}
                      />
                      <Tooltip
                        formatter={(value, name) => {
                          if (name === 'bytes') {
                            // Convert bytes to GB for display
                            const gb = value / (1024 * 1024 * 1024);
                            return [`${gb.toFixed(2)} GB`, 'Data Transfer'];
                          } else if (name === 'messages') {
                            // Format messages with K for thousands
                            return [`${(value / 1000).toFixed(2)}K`, 'Messages'];
                          }
                          return [value, name];
                        }}
                        labelFormatter={(label) => `Date: ${label}`}
                        contentStyle={{
                          backgroundColor: 'var(--background)',
                          border: '1px solid var(--border)',
                          borderRadius: '8px'
                        }}
                      />
                      {displayDataTypes.messages && (
                        <Area
                          yAxisId="right"
                          type="monotone"
                          dataKey="messages"
                          stroke="#10b981"
                          strokeWidth={2}
                          fillOpacity={1}
                          fill="url(#colorMessages)"
                          name="Messages"
                        />
                      )}
                      {displayDataTypes.dataTransfer && (
                        <Area
                          yAxisId="left"
                          type="monotone"
                          dataKey="bytes"
                          stroke="#3b82f6"
                          strokeWidth={2}
                          fillOpacity={1}
                          fill="url(#colorDataTransfer)"
                          name="Data Transfer"
                        />
                      )}
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
              </Card>
            </motion.div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.3 }}
            >
              <Card className="p-6 h-full flex flex-col">
                <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">Recent Activity</h2>
                <div className="space-y-4 flex-grow">
                  {recentActivity.map((activity) => (
                    <div key={activity.text} className="flex items-start gap-3">
                      <div className="w-2 h-2 rounded-full bg-blue-600 mt-2" />
                      <div className="flex-1">
                        <p className="text-sm text-gray-900 dark:text-white">{activity.text}</p>
                        <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">{activity.time}</p>
                      </div>
                    </div>
                  ))}
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
            <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">Quick Actions</h2>
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
              {quickActions.map((action, index) => (
                <Link key={action.title} href={action.link}>
                  <Card className="p-6 h-full hover:shadow-lg transition-all cursor-pointer group flex flex-col">
                    <div className={`w-12 h-12 rounded-lg ${action.color} flex items-center justify-center mb-4 group-hover:scale-110 transition-transform`}>
                      <action.icon className="w-6 h-6 text-white" />
                    </div>
                    <h3 className="font-semibold text-gray-900 dark:text-white mb-1">{action.title}</h3>
                    <p className="text-sm text-gray-600 dark:text-gray-400 flex-grow">{action.description}</p>
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
          className="flex items-center justify-center py-20"
        >
          <Card className="p-12 text-center max-w-md">
            <div className="w-20 h-20 bg-blue-100 dark:bg-blue-900/20 rounded-full flex items-center justify-center mx-auto mb-6">
              <Server className="w-10 h-10 text-blue-600" />
            </div>
            <h2 className="text-2xl font-bold mb-2 text-gray-900 dark:text-white">
              Ready to build something great?
            </h2>
            <p className="text-gray-600 dark:text-gray-400 mb-6">
              Create your first application to start sending secure messages
            </p>
            <Link href="/applications/new">
              <Button size="lg" className="bg-blue-600 hover:bg-blue-700">
                <Plus className="w-5 h-5 mr-2" />
                Create Your First Application
              </Button>
            </Link>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-4">
              No coding required • 2 minutes to setup
            </p>
          </Card>
        </motion.div>
      )}
    </DashboardLayout>
  );
}
