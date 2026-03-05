"use client";
import { useState, useEffect } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import { ArrowLeft, Edit, BarChart3, Key, Webhook, Settings as SettingsIcon, CreditCard, Zap, AlertTriangle, Check, Copy, RefreshCw, Plus, Trash2 } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Progress } from '../components/ui/progress';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '../components/ui/dialog';
import DashboardLayout from '../components/layout/DashboardLayout';
import { formatRelativeTime } from '@/lib/date';
import { applicationsApi } from '@/lib/api/applications';
import type { ApplicationDashboardResponse, AttachedPricingPlan } from '@/types/api';

// Available plans
const plans = [
  {
    name: 'Free',
    price: 0,
    features: ['10,000 messages/month', '5 GB bandwidth', '1 GB storage', '2 API keys'],
  },
  {
    name: 'Pro',
    price: 29,
    features: ['100,000 messages/month', '50 GB bandwidth', '10 GB storage', '10 API keys', 'Priority support'],
  },
  {
    name: 'Enterprise',
    price: 99,
    features: ['Unlimited messages', '500 GB bandwidth', '100 GB storage', 'Unlimited API keys', '24/7 support', 'Custom integrations'],
  }
];

export default function ApplicationDetailPage() {
  const params = useParams();
  const id = params?.id as string | undefined;
  
  // State
  const [data, setData] = useState<ApplicationDashboardResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [cancelDialogOpen, setCancelDialogOpen] = useState(false);
  const [upgradeDialogOpen, setUpgradeDialogOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  // Fetch data
  const fetchData = async () => {
    if (!id) return;
    setLoading(true);
    setError(null);
    try {
      const response = await applicationsApi.getAnalytics(id);
      setData(response);
    } catch (err: any) {
      setError(err.message || 'Failed to load application data');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
  }, [id]);

  const handleRefresh = async () => {
    setRefreshing(true);
    await fetchData();
    setRefreshing(false);
  };

  if (loading) {
    return (
      <DashboardLayout>
        <div className="flex items-center justify-center min-h-[60vh]">
          <div className="text-center">
            <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-600 mx-auto mb-4" />
            <p className="text-gray-600 dark:text-gray-400">Loading application data...</p>
          </div>
        </div>
      </DashboardLayout>
    );
  }

  if (error || !data) {
    return (
      <DashboardLayout>
        <div className="flex items-center justify-center min-h-[60vh]">
          <div className="text-center">
            <AlertTriangle className="w-12 h-12 text-red-500 mx-auto mb-4" />
            <h2 className="text-xl font-semibold text-gray-900 dark:text-white mb-2">Failed to Load Data</h2>
            <p className="text-gray-600 dark:text-gray-400 mb-4">{error || 'Application not found'}</p>
            <Button onClick={fetchData}>Try Again</Button>
          </div>
        </div>
      </DashboardLayout>
    );
  }

  const currentPlan = plans.find(p => p.name === (data.tier || 'Free')) || plans[0];
  const isOverQuota = data.quotaStatus.isOverQuota || data.quotaStatus.usagePct >= 100;
  const isApproachingQuota = data.quotaStatus.usagePct >= 80 && !isOverQuota;

  return (
    <DashboardLayout>
      <div className="mb-6">
        <Link href="/applications" className="inline-flex items-center text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white mb-4">
          <ArrowLeft className="w-4 h-4 mr-2" />
          Back to Apps
        </Link>

        <div className="flex items-start justify-between">
          <div>
            <div className="flex items-center gap-3 mb-2">
              <h1 className="text-3xl font-bold text-gray-900 dark:text-white">{data.name}</h1>
              <Badge>{data.tier || 'Free'}</Badge>
              <div className="flex items-center gap-2">
                <div className={`w-2 h-2 rounded-full ${data.active ? 'bg-green-500' : 'bg-gray-500'}`} />
                <span className="text-sm text-gray-600 dark:text-gray-400 capitalize">{data.active ? 'active' : 'inactive'}</span>
              </div>
            </div>
            {data.desc && (
              <p className="text-gray-600 dark:text-gray-400">{data.desc}</p>
            )}
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
              Created {formatRelativeTime(data.created)}
            </p>
          </div>

          <div className="flex gap-2">
            <Button variant="outline" onClick={handleRefresh} disabled={refreshing}>
              <RefreshCw className={`w-4 h-4 mr-2 ${refreshing ? 'animate-spin' : ''}`} />
              Refresh
            </Button>
            <Button variant="outline">
              <Edit className="w-4 h-4 mr-2" />
              Edit
            </Button>
            <Button variant="outline" onClick={() => setUpgradeDialogOpen(true)}>
              <Zap className="w-4 h-4 mr-2" />
              Change Plan
            </Button>
          </div>
        </div>
      </div>

      <Tabs defaultValue="overview" className="space-y-6">
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="plan">Plan & Billing</TabsTrigger>
          <TabsTrigger value="analytics">Analytics</TabsTrigger>
          <TabsTrigger value="keys">API Keys</TabsTrigger>
          <TabsTrigger value="webhooks">Webhooks</TabsTrigger>
          <TabsTrigger value="settings">Settings</TabsTrigger>
        </TabsList>

        {/* Overview Tab */}
        <TabsContent value="overview" className="space-y-6">
          <div className="grid md:grid-cols-2 lg:grid-cols-4 gap-6">
            <Card className="p-6">
              <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Messages Today</div>
              <div className="text-3xl font-bold text-gray-900 dark:text-white">
                {data.currentMonth.msgSent.toLocaleString()}
              </div>
              <div className="text-sm text-gray-600 dark:text-gray-400 mt-1">This month</div>
            </Card>

            <Card className="p-6">
              <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Bandwidth</div>
              <div className="text-3xl font-bold text-gray-900 dark:text-white">
                {((data.currentMonth.bytesSent + data.currentMonth.bytesReceived) / (1024 * 1024 * 1024)).toFixed(2)} GB
              </div>
              <div className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                of {data.tier === 'Enterprise' ? '500' : data.tier === 'Pro' ? '50' : '5'} GB
              </div>
            </Card>

            <Card className="p-6">
              <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Active Clients</div>
              <div className="text-3xl font-bold text-gray-900 dark:text-white">
                {data.billableClientsCount}
              </div>
              <div className="text-sm text-green-600 mt-1">
                {data.pricingPlan ? data.pricingPlan.planName : 'No plan'}
              </div>
            </Card>

            <Card className="p-6">
              <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">This Month</div>
              <div className="text-3xl font-bold text-gray-900 dark:text-white">
                ${(data.currentMonthRevenueCents / 100).toFixed(2)}
              </div>
              <div className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                Revenue generated
              </div>
            </Card>
          </div>

          <Card className="p-6">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Quota Usage</h2>
              <Badge variant={isOverQuota ? 'destructive' : isApproachingQuota ? 'secondary' : 'default'}>
                {data.tier || 'Free'} Plan
              </Badge>
            </div>
            <div className="mb-4">
              <div className="flex items-center justify-between mb-2">
                <span className="text-sm text-gray-600 dark:text-gray-400">Messages</span>
                <span className={`text-sm font-medium ${isOverQuota ? 'text-red-600' : isApproachingQuota ? 'text-yellow-600' : 'text-gray-900 dark:text-white'}`}>
                  {data.quotaUsagePct.toFixed(1)}%
                </span>
              </div>
              <Progress value={Math.min(data.quotaUsagePct, 100)} className="h-3" />
              <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                {data.quotaStatus.messagesUsed.toLocaleString()} / {data.monthlyQuota?.toLocaleString() || 'Unlimited'} messages
              </p>
            </div>

            {isApproachingQuota && (
              <div className="mt-4 p-4 bg-yellow-50 dark:bg-yellow-900/20 rounded-lg">
                <p className="text-sm text-yellow-800 dark:text-yellow-200">
                  You're using over 80% of your quota. Consider upgrading to avoid interruptions.
                </p>
                <Button size="sm" className="mt-2" onClick={() => setUpgradeDialogOpen(true)}>
                  Upgrade Plan
                </Button>
              </div>
            )}

            {isOverQuota && (
              <div className="mt-4 p-4 bg-red-50 dark:bg-red-900/20 rounded-lg">
                <p className="text-sm text-red-800 dark:text-red-200">
                  You've exceeded your quota! {data.quotaStatus.overageCount.toLocaleString()} messages over limit.
                </p>
                <Button size="sm" className="mt-2" variant="destructive" onClick={() => setUpgradeDialogOpen(true)}>
                  Upgrade Now
                </Button>
              </div>
            )}
          </Card>

          {/* Usage Trends */}
          <Card className="p-6">
            <h2 className="text-xl font-semibold text-gray-900 dark:text-white mb-4">Usage Trends</h2>
            <div className="grid md:grid-cols-3 gap-4">
              <div>
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Daily Average</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">
                  {data.trends.dailyAvgMessages.toLocaleString()}
                </div>
                <div className="text-sm text-gray-600 dark:text-gray-400">messages/day</div>
              </div>
              <div>
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Projected Monthly</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">
                  {data.trends.projectedMonthlyMessages.toLocaleString()}
                </div>
                <div className="text-sm text-gray-600 dark:text-gray-400">messages</div>
              </div>
              <div>
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Trend</div>
                <div className={`text-2xl font-bold capitalize ${
                  data.trends.quotaTrend === 'critical' ? 'text-red-600' :
                  data.trends.quotaTrend === 'increasing' ? 'text-yellow-600' :
                  'text-green-600'
                }`}>
                  {data.trends.quotaTrend}
                </div>
                <div className="text-sm text-gray-600 dark:text-gray-400">quota trend</div>
              </div>
            </div>
          </Card>
        </TabsContent>

        {/* Plan & Billing Tab */}
        <TabsContent value="plan" className="space-y-6">
          {/* Current Plan */}
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <div>
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Current Plan</h2>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                  Manage your subscription for this application
                </p>
              </div>
              <Button variant="outline" onClick={() => setUpgradeDialogOpen(true)}>
                <Zap className="w-4 h-4 mr-2" />
                Change Plan
              </Button>
            </div>

            <div className="flex items-center gap-4 p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
              <div className="w-16 h-16 bg-gradient-to-br from-blue-500 to-purple-600 rounded-xl flex items-center justify-center">
                <Zap className="w-8 h-8 text-white" />
              </div>
              <div className="flex-1">
                <div className="flex items-center gap-3">
                  <h3 className="text-xl font-bold text-gray-900 dark:text-white">{currentPlan.name}</h3>
                  <Badge className="bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400">
                    Current Plan
                  </Badge>
                </div>
                <p className="text-gray-600 dark:text-gray-400">
                  ${currentPlan.price}/month
                </p>
              </div>
            </div>

            <div className="mt-6">
              <h4 className="font-medium text-gray-900 dark:text-white mb-3">Plan Features</h4>
              <ul className="space-y-2">
                {currentPlan.features.map((feature, index) => (
                  <li key={index} className="flex items-center gap-2 text-gray-600 dark:text-gray-400">
                    <Check className="w-4 h-4 text-green-500" />
                    {feature}
                  </li>
                ))}
              </ul>
            </div>
          </Card>

          {/* Usage This Month */}
          <Card className="p-6">
            <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">Usage This Month</h2>
            <div className="grid md:grid-cols-3 gap-6">
              <div>
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm text-gray-600 dark:text-gray-400">Messages</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white">
                    {data.currentMonth.msgSent.toLocaleString()}
                  </span>
                </div>
                <Progress value={data.quotaUsagePct} className="h-3" />
                <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                  of {data.monthlyQuota?.toLocaleString() || 'Unlimited'}
                </p>
              </div>
              <div>
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm text-gray-600 dark:text-gray-400">Bandwidth</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white">
                    {((data.currentMonth.bytesSent + data.currentMonth.bytesReceived) / (1024 * 1024 * 1024)).toFixed(2)} GB
                  </span>
                </div>
                <Progress value={Math.min(((data.currentMonth.bytesSent + data.currentMonth.bytesReceived) / (1024 * 1024 * 1024)) / (data.tier === 'Enterprise' ? 500 : data.tier === 'Pro' ? 50 : 5) * 100, 100)} className="h-3" />
                <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                  of {data.tier === 'Enterprise' ? '500 GB' : data.tier === 'Pro' ? '50 GB' : '5 GB'}
                </p>
              </div>
              <div>
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm text-gray-600 dark:text-gray-400">Storage</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white">
                    {(data.currentMonth.msgStored / (1024 * 1024)).toFixed(2)} MB
                  </span>
                </div>
                <Progress value={Math.min((data.currentMonth.msgStored / (1024 * 1024)) / (data.tier === 'Enterprise' ? 100 : data.tier === 'Pro' ? 10 : 1) * 100, 100)} className="h-3" />
                <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                  of {data.tier === 'Enterprise' ? '100 GB' : data.tier === 'Pro' ? '10 GB' : '1 GB'}
                </p>
              </div>
            </div>
          </Card>

          {/* Billing History */}
          <Card className="p-6">
            <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">Billing History</h2>
            <div className="overflow-x-auto">
              <table className="w-full">
                <thead className="bg-gray-50 dark:bg-gray-800/50">
                  <tr>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-600 dark:text-gray-400 uppercase">Date</th>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-600 dark:text-gray-400 uppercase">Description</th>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-600 dark:text-gray-400 uppercase">Amount</th>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-600 dark:text-gray-400 uppercase">Status</th>
                    <th className="px-4 py-3 text-right text-xs font-medium text-gray-600 dark:text-gray-400 uppercase">Invoice</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-gray-700">
                  <tr>
                    <td className="px-4 py-3 text-sm text-gray-900 dark:text-white">Dec 1, 2024</td>
                    <td className="px-4 py-3 text-sm text-gray-600 dark:text-gray-400">{data.name} - {currentPlan.name} Plan</td>
                    <td className="px-4 py-3 text-sm font-medium text-gray-900 dark:text-white">${currentPlan.price}.00</td>
                    <td className="px-4 py-3"><Badge variant="secondary" className="bg-green-100 text-green-700">Paid</Badge></td>
                    <td className="px-4 py-3 text-right"><Button variant="ghost" size="sm">PDF</Button></td>
                  </tr>
                  <tr>
                    <td className="px-4 py-3 text-sm text-gray-900 dark:text-white">Nov 1, 2024</td>
                    <td className="px-4 py-3 text-sm text-gray-600 dark:text-gray-400">{data.name} - {currentPlan.name} Plan</td>
                    <td className="px-4 py-3 text-sm font-medium text-gray-900 dark:text-white">${currentPlan.price}.00</td>
                    <td className="px-4 py-3"><Badge variant="secondary" className="bg-green-100 text-green-700">Paid</Badge></td>
                    <td className="px-4 py-3 text-right"><Button variant="ghost" size="sm">PDF</Button></td>
                  </tr>
                  <tr>
                    <td className="px-4 py-3 text-sm text-gray-900 dark:text-white">Oct 1, 2024</td>
                    <td className="px-4 py-3 text-sm text-gray-600 dark:text-gray-400">{data.name} - {currentPlan.name} Plan</td>
                    <td className="px-4 py-3 text-sm font-medium text-gray-900 dark:text-white">${currentPlan.price}.00</td>
                    <td className="px-4 py-3"><Badge variant="secondary" className="bg-green-100 text-green-700">Paid</Badge></td>
                    <td className="px-4 py-3 text-right"><Button variant="ghost" size="sm">PDF</Button></td>
                  </tr>
                </tbody>
              </table>
            </div>
          </Card>

          {/* Danger Zone */}
          <Card className="p-6 border-red-200 dark:border-red-800">
            <h2 className="text-xl font-semibold text-red-600 mb-4">Danger Zone</h2>
            <div className="flex items-center justify-between p-4 bg-red-50 dark:bg-red-900/10 rounded-lg">
              <div>
                <p className="font-medium text-gray-900 dark:text-white">Cancel Subscription</p>
                <p className="text-sm text-gray-600 dark:text-gray-400">
                  This will stop billing for this application. Your data will be retained for 30 days.
                </p>
              </div>
              <Dialog open={cancelDialogOpen} onOpenChange={setCancelDialogOpen}>
                <DialogTrigger asChild>
                  <Button variant="destructive">Cancel Subscription</Button>
                </DialogTrigger>
                <DialogContent>
                  <DialogHeader>
                    <DialogTitle>Cancel Subscription</DialogTitle>
                    <DialogDescription>
                      Are you sure you want to cancel the subscription for <strong>{data.name}</strong>?
                      This will downgrade the application to the Free plan at the end of the current billing period.
                    </DialogDescription>
                  </DialogHeader>
                  <div className="py-4">
                    <div className="p-4 bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800 rounded-lg">
                      <div className="flex items-start gap-3">
                        <AlertTriangle className="w-5 h-5 text-yellow-600 flex-shrink-0 mt-0.5" />
                        <div className="text-sm text-yellow-800 dark:text-yellow-200">
                          <p className="font-semibold mb-1">Warning</p>
                          <p>After cancellation, this application will be downgraded to the Free plan with limited quotas.</p>
                        </div>
                      </div>
                    </div>
                  </div>
                  <DialogFooter>
                    <Button variant="outline" onClick={() => setCancelDialogOpen(false)}>Keep Subscription</Button>
                    <Button variant="destructive" onClick={() => setCancelDialogOpen(false)}>Confirm Cancellation</Button>
                  </DialogFooter>
                </DialogContent>
              </Dialog>
            </div>
          </Card>
        </TabsContent>

        {/* Analytics Tab */}
        <TabsContent value="analytics">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <div>
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white mb-1">
                  Analytics
                </h2>
                <p className="text-sm text-gray-600 dark:text-gray-400">
                  View detailed analytics for this application
                </p>
              </div>
              <Link href={`/applications/${id}/analytics`}>
                <Button>
                  <BarChart3 className="w-4 h-4 mr-2" />
                  Open Full Analytics
                </Button>
              </Link>
            </div>
            <div className="grid md:grid-cols-3 gap-4">
              <Card className="p-4">
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Messages This Month</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">
                  {data.currentMonth.msgSent.toLocaleString()}
                </div>
              </Card>
              <Card className="p-4">
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Proofs Verified</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">
                  {data.currentMonth.msgProof.toLocaleString()}
                </div>
              </Card>
              <Card className="p-4">
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Rate Limit Hits</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">
                  {data.currentMonth.rateHits.toLocaleString()}
                </div>
              </Card>
            </div>
          </Card>
        </TabsContent>

        {/* API Keys Tab */}
        <TabsContent value="keys">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <div>
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white mb-1">
                  Secret Key
                </h2>
                <p className="text-sm text-gray-600 dark:text-gray-400">
                  Your secret key for server-side authentication
                </p>
              </div>
              <Button variant="outline">
                <RefreshCw className="w-4 h-4 mr-2" />
                Rotate Key
              </Button>
            </div>

            {data.secretKeyPrefix && (
              <div className="p-4 bg-gray-50 dark:bg-gray-800 rounded-lg mb-6">
                <div className="flex items-center justify-between">
                  <div>
                    <p className="text-sm text-gray-600 dark:text-gray-400 mb-1">Secret Key Prefix</p>
                    <code className="text-lg font-mono text-gray-900 dark:text-white">{data.secretKeyPrefix}...</code>
                  </div>
                  <Badge variant={data.secretKeyIsActive ? 'default' : 'secondary'}>
                    {data.secretKeyIsActive ? 'Active' : 'Inactive'}
                  </Badge>
                </div>
                {data.secretKeyScopes && (
                  <p className="text-sm text-gray-600 dark:text-gray-400 mt-2">
                    Scopes: <code className="text-xs">{data.secretKeyScopes}</code>
                  </p>
                )}
              </div>
            )}

            <div className="border-t pt-6">
              <div className="flex items-center justify-between mb-4">
                <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
                  Publishable Keys ({data.keys.length})
                </h3>
                <Button>
                  <Plus className="w-4 h-4 mr-2" />
                  Add Key
                </Button>
              </div>

              {data.keys.length === 0 ? (
                <p className="text-gray-600 dark:text-gray-400 text-sm">No publishable keys configured.</p>
              ) : (
                <div className="space-y-3">
                  {data.keys.map((key) => (
                    <div key={key.id} className="p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
                      <div className="flex items-center justify-between">
                        <div>
                          <code className="text-sm font-mono text-gray-900 dark:text-white">{key.keyPrefix}...</code>
                          <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                            Created {formatRelativeTime(key.createdAt)}
                          </p>
                        </div>
                        <div className="flex items-center gap-2">
                          <Badge variant={key.isActive ? 'default' : 'secondary'}>
                            {key.isActive ? 'Active' : 'Inactive'}
                          </Badge>
                          <Button variant="ghost" size="sm">
                            <Copy className="w-4 h-4" />
                          </Button>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </Card>
        </TabsContent>

        {/* Webhooks Tab */}
        <TabsContent value="webhooks">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <div>
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white mb-1">
                  Webhooks
                </h2>
                <p className="text-sm text-gray-600 dark:text-gray-400">
                  Configure webhook endpoints for real-time notifications
                </p>
              </div>
              <Button>
                <Plus className="w-4 h-4 mr-2" />
                Add Webhook
              </Button>
            </div>

            {data.webhooks.length === 0 ? (
              <div className="text-center py-8">
                <Webhook className="w-12 h-12 text-gray-400 mx-auto mb-4" />
                <p className="text-gray-600 dark:text-gray-400">No webhooks configured</p>
                <p className="text-sm text-gray-500 dark:text-gray-500 mt-1">
                  Add a webhook to receive real-time notifications
                </p>
              </div>
            ) : (
              <div className="space-y-3">
                {data.webhooks.map((webhook) => (
                  <div key={webhook.id} className="p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
                    <div className="flex items-center justify-between">
                      <div className="flex-1">
                        <div className="flex items-center gap-2">
                          <code className="text-sm font-mono text-gray-900 dark:text-white">{webhook.url}</code>
                          <Badge variant={webhook.isActive ? 'default' : 'secondary'}>
                            {webhook.isActive ? 'Active' : 'Inactive'}
                          </Badge>
                        </div>
                        <div className="flex items-center gap-2 mt-2">
                          <span className="text-xs text-gray-600 dark:text-gray-400">Events:</span>
                          <div className="flex gap-1">
                            {webhook.events.map((event) => (
                              <Badge key={event} variant="outline" className="text-xs">
                                {event}
                              </Badge>
                            ))}
                          </div>
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <Button variant="ghost" size="sm">
                          <Edit className="w-4 h-4" />
                        </Button>
                        <Button variant="ghost" size="sm">
                          <Trash2 className="w-4 h-4 text-red-500" />
                        </Button>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </Card>
        </TabsContent>

        {/* Settings Tab */}
        <TabsContent value="settings">
          <Card className="p-6">
            <h2 className="text-xl font-semibold text-gray-900 dark:text-white mb-6">
              Application Settings
            </h2>
            
            <div className="space-y-6">
              <div>
                <h3 className="text-lg font-semibold text-gray-900 dark:text-white mb-4">General</h3>
                <div className="space-y-4">
                  <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
                    <div>
                      <p className="font-medium text-gray-900 dark:text-white">Application Name</p>
                      <p className="text-sm text-gray-600 dark:text-gray-400">{data.name}</p>
                    </div>
                    <Button variant="outline" size="sm">Edit</Button>
                  </div>
                  <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
                    <div>
                      <p className="font-medium text-gray-900 dark:text-white">Status</p>
                      <p className="text-sm text-gray-600 dark:text-gray-400 capitalize">{data.active ? 'Active' : 'Inactive'}</p>
                    </div>
                    <Button variant="outline" size="sm">Toggle</Button>
                  </div>
                </div>
              </div>

              <div className="border-t pt-6">
                <h3 className="text-lg font-semibold text-gray-900 dark:text-white mb-4">Advanced</h3>
                <div className="space-y-4">
                  <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
                    <div>
                      <p className="font-medium text-gray-900 dark:text-white">Rate Limit</p>
                      <p className="text-sm text-gray-600 dark:text-gray-400">{data.rateLimit || 'N/A'} requests/minute</p>
                    </div>
                    <Button variant="outline" size="sm">Configure</Button>
                  </div>
                  <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
                    <div>
                      <p className="font-medium text-gray-900 dark:text-white">Message Retention</p>
                      <p className="text-sm text-gray-600 dark:text-gray-400">
                        {data.retentionSeconds ? `${Math.floor(data.retentionSeconds / 86400)} days` : 'N/A'}
                      </p>
                    </div>
                    <Button variant="outline" size="sm">Configure</Button>
                  </div>
                </div>
              </div>

              <div className="border-t pt-6">
                <h3 className="text-lg font-semibold text-red-600 mb-4">Danger Zone</h3>
                <div className="p-4 bg-red-50 dark:bg-red-900/10 border border-red-200 dark:border-red-800 rounded-lg">
                  <div className="flex items-center justify-between">
                    <div>
                      <p className="font-medium text-gray-900 dark:text-white">Delete Application</p>
                      <p className="text-sm text-gray-600 dark:text-gray-400">
                        Permanently delete this application and all its data
                      </p>
                    </div>
                    <Button variant="destructive">Delete</Button>
                  </div>
                </div>
              </div>
            </div>
          </Card>
        </TabsContent>
      </Tabs>

      {/* Change Plan Dialog */}
      <Dialog open={upgradeDialogOpen} onOpenChange={setUpgradeDialogOpen}>
        <DialogContent className="max-w-3xl">
          <DialogHeader>
            <DialogTitle>Change Plan for {data.name}</DialogTitle>
            <DialogDescription>
              Choose a plan that best fits your needs
            </DialogDescription>
          </DialogHeader>
          <div className="grid md:grid-cols-3 gap-6 py-6">
            {plans.map((plan) => (
              <div
                key={plan.name}
                className={`p-6 rounded-lg border-2 transition-all ${
                  plan.name === data.tier
                    ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/10'
                    : 'border-gray-200 dark:border-gray-700'
                }`}
              >
                {plan.name === data.tier && (
                  <Badge className="mb-4 bg-blue-600">Current</Badge>
                )}
                <h3 className="text-xl font-bold text-gray-900 dark:text-white mb-2">
                  {plan.name}
                </h3>
                <div className="mb-4">
                  <span className="text-3xl font-bold text-gray-900 dark:text-white">
                    ${plan.price}
                  </span>
                  <span className="text-gray-600 dark:text-gray-400">/month</span>
                </div>
                <ul className="space-y-2 mb-6">
                  {plan.features.map((feature, index) => (
                    <li key={index} className="flex items-start gap-2 text-sm text-gray-600 dark:text-gray-400">
                      <Check className="w-4 h-4 text-green-500 flex-shrink-0 mt-0.5" />
                      {feature}
                    </li>
                  ))}
                </ul>
                <Button
                  className={`w-full ${
                    plan.name === data.tier
                      ? 'bg-gray-100 text-gray-400 cursor-not-allowed dark:bg-gray-800'
                      : 'bg-blue-600 hover:bg-blue-700'
                  }`}
                  disabled={plan.name === data.tier}
                  onClick={() => setUpgradeDialogOpen(false)}
                >
                  {plan.name === data.tier ? 'Current Plan' : plan.price > currentPlan.price ? 'Upgrade' : 'Downgrade'}
                </Button>
              </div>
            ))}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setUpgradeDialogOpen(false)}>Cancel</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </DashboardLayout>
  );
}
