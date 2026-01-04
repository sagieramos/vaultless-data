"use client";

import { useState } from 'react';
import { motion } from 'motion/react';
import {
  DollarSign, TrendingUp, TrendingDown, Users, CreditCard, Download,
  Calendar, ArrowUpRight, ArrowDownRight, Wallet, Banknote, Clock
} from 'lucide-react';
import DashboardLayout from '../components/layout/DashboardLayout';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs';
import {
  LineChart, Line, AreaChart, Area, XAxis, YAxis, CartesianGrid,
  Tooltip, ResponsiveContainer
} from 'recharts';

// Revenue data
const revenueData = [
  { date: 'Dec 1', revenue: 42, subscribers: 120 },
  { date: 'Dec 2', revenue: 38, subscribers: 122 },
  { date: 'Dec 3', revenue: 55, subscribers: 125 },
  { date: 'Dec 4', revenue: 48, subscribers: 128 },
  { date: 'Dec 5', revenue: 62, subscribers: 132 },
  { date: 'Dec 6', revenue: 45, subscribers: 135 },
  { date: 'Dec 7', revenue: 58, subscribers: 138 },
  { date: 'Dec 8', revenue: 72, subscribers: 140 },
  { date: 'Dec 9', revenue: 65, subscribers: 142 },
  { date: 'Dec 10', revenue: 78, subscribers: 145 },
  { date: 'Dec 11', revenue: 82, subscribers: 148 },
  { date: 'Dec 12', revenue: 75, subscribers: 150 },
  { date: 'Dec 13', revenue: 88, subscribers: 152 },
  { date: 'Dec 14', revenue: 95, subscribers: 155 },
];

// Subscription plans breakdown
const subscriptionBreakdown = [
  { plan: 'Starter ($9/mo)', subscribers: 89, revenue: 801, percentage: 58 },
  { plan: 'Pro ($29/mo)', subscribers: 52, revenue: 1508, percentage: 34 },
  { plan: 'Enterprise ($99/mo)', subscribers: 14, revenue: 1386, percentage: 8 },
];

// Payout history
const payoutHistory = [
  { id: '1', date: 'Dec 1, 2024', amount: 892.45, status: 'completed', method: 'Bank Transfer' },
  { id: '2', date: 'Nov 1, 2024', amount: 756.32, status: 'completed', method: 'Bank Transfer' },
  { id: '3', date: 'Oct 1, 2024', amount: 684.18, status: 'completed', method: 'Bank Transfer' },
  { id: '4', date: 'Sep 1, 2024', amount: 512.50, status: 'completed', method: 'Bank Transfer' },
  { id: '5', date: 'Aug 1, 2024', amount: 428.75, status: 'completed', method: 'Bank Transfer' },
];

// Recent subscriptions
const recentSubscriptions = [
  { id: '1', user: 'john@example.com', plan: 'Pro', amount: 29, date: '2 hours ago', status: 'active' },
  { id: '2', user: 'sarah@company.io', plan: 'Enterprise', amount: 99, date: '5 hours ago', status: 'active' },
  { id: '3', user: 'mike@startup.co', plan: 'Starter', amount: 9, date: '1 day ago', status: 'active' },
  { id: '4', user: 'lisa@tech.io', plan: 'Pro', amount: 29, date: '2 days ago', status: 'active' },
  { id: '5', user: 'tom@web.dev', plan: 'Starter', amount: 9, date: '3 days ago', status: 'churned' },
];

export default function RevenuePage() {
  const [dateRange, setDateRange] = useState('30d');

  const totalRevenue = revenueData.reduce((sum, d) => sum + d.revenue, 0);
  const totalSubscribers = revenueData[revenueData.length - 1].subscribers;
  const avgRevenuePerSubscriber = totalRevenue / totalSubscribers;
  const previousPeriodRevenue = totalRevenue * 0.82;
  const revenueGrowth = ((totalRevenue - previousPeriodRevenue) / previousPeriodRevenue) * 100;

  return (
    <DashboardLayout>
      {/* Header */}
      <div className="mb-8 flex flex-col md:flex-row md:items-center md:justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white">Revenue</h1>
          <p className="text-gray-600 dark:text-gray-400 mt-1">
            Track your earnings from client subscriptions
          </p>
        </div>
        <div className="flex items-center gap-3">
          <Button variant="outline">
            <Calendar className="w-4 h-4 mr-2" />
            Last 30 Days
          </Button>
          <Button>
            <Wallet className="w-4 h-4 mr-2" />
            Request Payout
          </Button>
        </div>
      </div>

      {/* Stats Cards */}
      <div className="grid md:grid-cols-2 lg:grid-cols-4 gap-6 mb-8">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
        >
          <Card className="p-6 border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/10">
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-green-700 dark:text-green-400">Total Revenue</span>
              <DollarSign className="w-5 h-5 text-green-600" />
            </div>
            <div className="flex items-end gap-2">
              <span className="text-3xl font-bold text-green-800 dark:text-green-300">
                ${totalRevenue.toFixed(2)}
              </span>
              <span className={`text-sm flex items-center mb-1 ${revenueGrowth >= 0 ? 'text-green-600' : 'text-red-600'}`}>
                {revenueGrowth >= 0 ? <ArrowUpRight className="w-4 h-4 mr-1" /> : <ArrowDownRight className="w-4 h-4 mr-1" />}
                {revenueGrowth >= 0 ? '+' : ''}{revenueGrowth.toFixed(1)}%
              </span>
            </div>
            <p className="text-sm text-green-600 dark:text-green-500 mt-1">vs last period</p>
          </Card>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.2 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Active Subscribers</span>
              <Users className="w-5 h-5 text-purple-600" />
            </div>
            <div className="flex items-end gap-2">
              <span className="text-3xl font-bold text-gray-900 dark:text-white">
                {totalSubscribers}
              </span>
              <span className="text-sm text-green-600 flex items-center mb-1">
                <ArrowUpRight className="w-4 h-4 mr-1" />
                +28
              </span>
            </div>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">+12% from last month</p>
          </Card>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.3 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-gray-600 dark:text-gray-400">ARPU</span>
              <CreditCard className="w-5 h-5 text-blue-600" />
            </div>
            <div className="flex items-end gap-2">
              <span className="text-3xl font-bold text-gray-900 dark:text-white">
                ${avgRevenuePerSubscriber.toFixed(2)}
              </span>
              <span className="text-sm text-green-600 flex items-center mb-1">
                <ArrowUpRight className="w-4 h-4 mr-1" />
                +5%
              </span>
            </div>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">Avg revenue per user</p>
          </Card>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.4 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Churn Rate</span>
              <TrendingDown className="w-5 h-5 text-red-600" />
            </div>
            <div className="flex items-end gap-2">
              <span className="text-3xl font-bold text-gray-900 dark:text-white">
                2.4%
              </span>
              <span className="text-sm text-green-600 flex items-center mb-1">
                <TrendingDown className="w-4 h-4 mr-1" />
                -0.5%
              </span>
            </div>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">This month</p>
          </Card>
        </motion.div>
      </div>

      <Tabs defaultValue="overview" className="space-y-6">
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="subscriptions">Subscriptions</TabsTrigger>
          <TabsTrigger value="payouts">Payouts</TabsTrigger>
        </TabsList>

        {/* Overview Tab */}
        <TabsContent value="overview" className="space-y-6">
          <div className="grid lg:grid-cols-3 gap-6">
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.1 }}
              className="lg:col-span-2"
            >
              <Card className="p-6">
                <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">
                  Revenue Trend
                </h2>
                <ResponsiveContainer width="100%" height={350}>
                  <AreaChart data={revenueData}>
                    <defs>
                      <linearGradient id="colorRevenueGreen" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor="#10b981" stopOpacity={0.3}/>
                        <stop offset="95%" stopColor="#10b981" stopOpacity={0}/>
                      </linearGradient>
                    </defs>
                    <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                    <XAxis dataKey="date" className="text-gray-600 dark:text-gray-400" />
                    <YAxis
                      className="text-gray-600 dark:text-gray-400"
                      tickFormatter={(value) => `$${value}`}
                    />
                    <Tooltip
                      contentStyle={{
                        backgroundColor: 'var(--background)',
                        border: '1px solid var(--border)',
                        borderRadius: '8px'
                      }}
                      formatter={(value?: number) => value === undefined ? ['—', 'Revenue'] : [`$${value.toFixed(2)}`, 'Revenue']}
                    />
                    <Area
                      type="monotone"
                      dataKey="revenue"
                      stroke="#10b981"
                      strokeWidth={2}
                      fillOpacity={1}
                      fill="url(#colorRevenueGreen)"
                    />
                  </AreaChart>
                </ResponsiveContainer>
              </Card>
            </motion.div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.2 }}
            >
              <Card className="p-6 h-full">
                <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">
                  Revenue by Plan
                </h2>
                <div className="space-y-4">
                  {subscriptionBreakdown.map((plan) => (
                    <div key={plan.plan}>
                      <div className="flex items-center justify-between mb-2">
                        <span className="text-sm font-medium text-gray-900 dark:text-white">
                          {plan.plan}
                        </span>
                        <span className="text-sm text-gray-600 dark:text-gray-400">
                          ${plan.revenue} ({plan.percentage}%)
                        </span>
                      </div>
                      <div className="h-3 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                        <div
                          className="h-full bg-green-500 rounded-full transition-all"
                          style={{ width: `${plan.percentage * 1.5}%` }}
                        />
                      </div>
                      <p className="text-xs text-gray-500 mt-1">{plan.subscribers} subscribers</p>
                    </div>
                  ))}
                </div>
                <div className="mt-6 pt-6 border-t border-gray-200 dark:border-gray-700">
                  <div className="flex items-center justify-between">
                    <span className="font-semibold text-gray-900 dark:text-white">Total</span>
                    <span className="text-xl font-bold text-green-600">
                      ${(subscriptionBreakdown.reduce((sum, p) => sum + p.revenue, 0)).toFixed(2)}
                    </span>
                  </div>
                </div>
              </Card>
            </motion.div>
          </div>

          {/* Quick Stats */}
          <div className="grid md:grid-cols-3 gap-6">
            <Card className="p-6">
              <h3 className="text-lg font-semibold mb-4 text-gray-900 dark:text-white">
                Best Selling Plan
              </h3>
              <p className="text-3xl font-bold text-gray-900 dark:text-white mb-2">
                Pro
              </p>
              <p className="text-sm text-gray-600 dark:text-gray-400">
                34% of revenue comes from Pro subscribers
              </p>
            </Card>

            <Card className="p-6">
              <h3 className="text-lg font-semibold mb-4 text-gray-900 dark:text-white">
                Lifetime Revenue
              </h3>
              <p className="text-3xl font-bold text-gray-900 dark:text-white mb-2">
                $12,847
              </p>
              <p className="text-sm text-gray-600 dark:text-gray-400">
                Since you started using Vaultless
              </p>
            </Card>

            <Card className="p-6">
              <h3 className="text-lg font-semibold mb-4 text-gray-900 dark:text-white">
                Pending Payout
              </h3>
              <p className="text-3xl font-bold text-green-600 mb-2">
                $892.45
              </p>
              <p className="text-sm text-gray-600 dark:text-gray-400">
                Next payout: Jan 1, 2025
              </p>
            </Card>
          </div>
        </TabsContent>

        {/* Subscriptions Tab */}
        <TabsContent value="subscriptions">
          <Card className="overflow-hidden">
            <div className="p-6 border-b border-gray-200 dark:border-gray-700">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">
                Recent Subscriptions
              </h2>
              <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                New and churning subscriptions
              </p>
            </div>
            <div className="overflow-x-auto">
              <table className="w-full">
                <thead className="bg-gray-50 dark:bg-gray-800/50">
                  <tr>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-600 dark:text-gray-400 uppercase tracking-wider">
                      User
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-600 dark:text-gray-400 uppercase tracking-wider">
                      Plan
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-600 dark:text-gray-400 uppercase tracking-wider">
                      Amount
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-600 dark:text-gray-400 uppercase tracking-wider">
                      Date
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-600 dark:text-gray-400 uppercase tracking-wider">
                      Status
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-gray-700">
                  {recentSubscriptions.map((sub) => (
                    <tr key={sub.id} className="hover:bg-gray-50 dark:hover:bg-gray-800/50">
                      <td className="px-6 py-4 whitespace-nowrap">
                        <div className="flex items-center">
                          <div className="w-8 h-8 bg-gray-100 dark:bg-gray-800 rounded-full flex items-center justify-center mr-3">
                            <Users className="w-4 h-4 text-gray-500" />
                          </div>
                          <span className="text-sm text-gray-900 dark:text-white">{sub.user}</span>
                        </div>
                      </td>
                      <td className="px-6 py-4 whitespace-nowrap">
                        <Badge variant="secondary">{sub.plan}</Badge>
                      </td>
                      <td className="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900 dark:text-white">
                        ${sub.amount}/mo
                      </td>
                      <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-600 dark:text-gray-400">
                        {sub.date}
                      </td>
                      <td className="px-6 py-4 whitespace-nowrap">
                        <Badge
                          variant={sub.status === 'active' ? 'secondary' : 'destructive'}
                          className={sub.status === 'active' ? 'bg-green-100 text-green-700' : ''}
                        >
                          {sub.status.charAt(0).toUpperCase() + sub.status.slice(1)}
                        </Badge>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </Card>
        </TabsContent>

        {/* Payouts Tab */}
        <TabsContent value="payouts" className="space-y-6">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <div>
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">
                  Payout History
                </h2>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                  Your past and upcoming payouts
                </p>
              </div>
              <Button variant="outline">
                <Download className="w-4 h-4 mr-2" />
                Export
              </Button>
            </div>

            <div className="space-y-4">
              {payoutHistory.map((payout) => (
                <div
                  key={payout.id}
                  className="flex items-center justify-between p-4 bg-gray-50 dark:bg-gray-800/50 rounded-lg"
                >
                  <div className="flex items-center gap-4">
                    <div className="w-10 h-10 bg-green-100 dark:bg-green-900/20 rounded-lg flex items-center justify-center">
                      <Banknote className="w-5 h-5 text-green-600" />
                    </div>
                    <div>
                      <p className="font-medium text-gray-900 dark:text-white">
                        ${payout.amount.toFixed(2)}
                      </p>
                      <p className="text-sm text-gray-600 dark:text-gray-400">
                        {payout.method} • {payout.date}
                      </p>
                    </div>
                  </div>
                  <Badge variant="secondary" className="bg-green-100 text-green-700">
                    {payout.status.charAt(0).toUpperCase() + payout.status.slice(1)}
                  </Badge>
                </div>
              ))}
            </div>

            <div className="mt-6 p-4 bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-lg">
              <div className="flex items-start gap-3">
                <Clock className="w-5 h-5 text-blue-600 flex-shrink-0 mt-0.5" />
                <div>
                  <h3 className="font-semibold text-blue-900 dark:text-blue-100 mb-1">
                    Next Payout Scheduled
                  </h3>
                  <p className="text-sm text-blue-800 dark:text-blue-200">
                    Your payout of <span className="font-semibold">$892.45</span> will be processed on
                    January 15, 2026 and should arrive within 2-3 business days.
                  </p>
                </div>
              </div>
            </div>
          </Card>

          <Card className="p-6">
            <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">
              Payout Settings
            </h2>
            <div className="grid md:grid-cols-2 gap-6">
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                  Payout Method
                </label>
                <select className="w-full px-3 py-2 border border-gray-300 dark:border-gray-700 rounded-lg bg-white dark:bg-gray-900">
                  <option>Bank Transfer (ACH)</option>
                  <option>PayPal</option>
                  <option>Wire Transfer</option>
                </select>
              </div>
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                  Payout Schedule
                </label>
                <select className="w-full px-3 py-2 border border-gray-300 dark:border-gray-700 rounded-lg bg-white dark:bg-gray-900">
                  <option>Weekly</option>
                  <option>Bi-weekly</option>
                  <option>Monthly</option>
                </select>
              </div>
            </div>
            <div className="mt-4">
              <Button>Update Settings</Button>
            </div>
          </Card>
        </TabsContent>
      </Tabs>
    </DashboardLayout>
  );
}
