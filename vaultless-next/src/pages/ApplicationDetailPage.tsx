"use client";
import { useState } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import { ArrowLeft, Edit, BarChart3, Key, Webhook, Settings as SettingsIcon, CreditCard, Zap, AlertTriangle, Check } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Progress } from '../components/ui/progress';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '../components/ui/dialog';
import DashboardLayout from '../components/layout/DashboardLayout';
import { formatRelativeTime } from '@/lib/date';

// Mock application data
const applicationsData: Record<string, { name: string; description: string; tier: string; status: string; createdAt: string; type: string; iotStats?: { devices: number; attested: number; trusted: number } }> = {
  '1': { name: 'Production API', description: 'Main production messaging service', tier: 'Pro', status: 'active', createdAt: '2025-11-07T12:00:00Z', type: 'messaging' },
  '2': { name: 'Staging Environment', description: 'Testing and staging deployment', tier: 'Free', status: 'active', createdAt: '2025-12-07T12:00:00Z', type: 'messaging' },
  '3': { name: 'Smart Home Hub', description: 'IoT device management and attestation', tier: 'Enterprise', status: 'active', createdAt: '2025-12-17T12:00:00Z', type: 'iot', iotStats: { devices: 847, attested: 842, trusted: 98 } },
  '4': { name: 'Mobile App Backend', description: 'iOS and Android messaging service', tier: 'Pro', status: 'active', createdAt: '2025-12-17T12:00:00Z', type: 'messaging' },
};

// Available plans
const plans = [
  {
    name: 'Free',
    price: 0,
    features: ['10,000 messages/month', '5 GB bandwidth', '1 GB storage', '2 API keys'],
    current: false
  },
  {
    name: 'Pro',
    price: 29,
    features: ['100,000 messages/month', '50 GB bandwidth', '10 GB storage', '10 API keys', 'Priority support'],
    current: true
  },
  {
    name: 'Enterprise',
    price: 99,
    features: ['Unlimited messages', '500 GB bandwidth', '100 GB storage', 'Unlimited API keys', '24/7 support', 'Custom integrations'],
    current: false
  }
];

export default function ApplicationDetailPage() {
  const params = useParams();
  const id = params?.id as string | undefined;
  const [cancelDialogOpen, setCancelDialogOpen] = useState(false);
  const [upgradeDialogOpen, setUpgradeDialogOpen] = useState(false);

  const appData = id ? applicationsData[id] : null;
  const appName = appData?.name || 'Application';
  const appDescription = appData?.description || '';
  const appTier = appData?.tier || 'Free';
  const appStatus = appData?.status || 'active';
  const appCreatedAt = appData?.createdAt || '';
  const appType = appData?.type || 'messaging';

  const currentPlan = plans.find(p => p.name === appTier) || plans[0];

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
              <h1 className="text-3xl font-bold text-gray-900 dark:text-white">{appName}</h1>
              <Badge>{appTier}</Badge>
              {appType === 'iot' && (
                <Badge variant="outline">IoT</Badge>
              )}
              <div className="flex items-center gap-2">
                <div className="w-2 h-2 rounded-full bg-green-500" />
                <span className="text-sm text-gray-600 dark:text-gray-400 capitalize">{appStatus}</span>
              </div>
            </div>
            <p className="text-gray-600 dark:text-gray-400">{appDescription}</p>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
              Created {(appData?.createdAt || (appData as any)?.created_at) ? formatRelativeTime(appData?.createdAt ?? (appData as any)?.created_at) : 'Unknown'}
            </p>
          </div>

          <div className="flex gap-2">
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
              <div className="text-3xl font-bold text-gray-900 dark:text-white">2,847</div>
              <div className="text-sm text-green-600 mt-1">+12% from yesterday</div>
            </Card>

            <Card className="p-6">
              <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Bandwidth</div>
              <div className="text-3xl font-bold text-gray-900 dark:text-white">12.4 GB</div>
              <div className="text-sm text-gray-600 dark:text-gray-400 mt-1">of {appTier === 'Enterprise' ? '500' : appTier === 'Pro' ? '50' : '5'} GB</div>
            </Card>

            <Card className="p-6">
              <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">
                {appType === 'iot' ? 'Devices' : 'Active Clients'}
              </div>
              <div className="text-3xl font-bold text-gray-900 dark:text-white">
                {appType === 'iot' ? appData?.iotStats?.devices : '142'}
              </div>
              <div className="text-sm text-green-600 mt-1">
                {appType === 'iot' ? `${appData?.iotStats?.trusted}% trusted` : '+8 new today'}
              </div>
            </Card>

            <Card className="p-6">
              <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">This Month</div>
              <div className="text-3xl font-bold text-gray-900 dark:text-white">${currentPlan.price}</div>
              <div className="text-sm text-gray-600 dark:text-gray-400 mt-1">{currentPlan.name} plan</div>
            </Card>
          </div>

          <Card className="p-6">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Quota Usage</h2>
              <Badge variant="secondary">{currentPlan.name} Plan</Badge>
            </div>
            <div className="mb-4">
              <div className="flex items-center justify-between mb-2">
                <span className="text-sm text-gray-600 dark:text-gray-400">Messages</span>
                <span className="text-sm font-medium text-gray-900 dark:text-white">65%</span>
              </div>
              <Progress value={65} className="h-3" />
              <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                65,000 / {appTier === 'Enterprise' ? 'Unlimited' : appTier === 'Pro' ? '100,000' : '10,000'} messages
              </p>
            </div>

            {65 > 80 && (
              <div className="mt-4 p-4 bg-yellow-50 dark:bg-yellow-900/20 rounded-lg">
                <p className="text-sm text-yellow-800 dark:text-yellow-200">
                  You're using over 80% of your quota. Consider upgrading to avoid interruptions.
                </p>
                <Button size="sm" className="mt-2" onClick={() => setUpgradeDialogOpen(true)}>
                  Upgrade Plan
                </Button>
              </div>
            )}
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
                  ${currentPlan.price}/month • Renews on Jan 1, 2025
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
                  <span className="text-sm font-medium text-gray-900 dark:text-white">65,000</span>
                </div>
                <Progress value={65} className="h-3" />
                <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                  of {appTier === 'Enterprise' ? 'Unlimited' : appTier === 'Pro' ? '100,000' : '10,000'}
                </p>
              </div>
              <div>
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm text-gray-600 dark:text-gray-400">Bandwidth</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white">12.4 GB</span>
                </div>
                <Progress value={25} className="h-3" />
                <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                  of {appTier === 'Enterprise' ? '500 GB' : appTier === 'Pro' ? '50 GB' : '5 GB'}
                </p>
              </div>
              <div>
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm text-gray-600 dark:text-gray-400">Storage</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white">2.4 GB</span>
                </div>
                <Progress value={24} className="h-3" />
                <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                  of {appTier === 'Enterprise' ? '100 GB' : appTier === 'Pro' ? '10 GB' : '1 GB'}
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
                    <td className="px-4 py-3 text-sm text-gray-600 dark:text-gray-400">{appName} - {currentPlan.name} Plan</td>
                    <td className="px-4 py-3 text-sm font-medium text-gray-900 dark:text-white">${currentPlan.price}.00</td>
                    <td className="px-4 py-3"><Badge variant="secondary" className="bg-green-100 text-green-700">Paid</Badge></td>
                    <td className="px-4 py-3 text-right"><Button variant="ghost" size="sm">PDF</Button></td>
                  </tr>
                  <tr>
                    <td className="px-4 py-3 text-sm text-gray-900 dark:text-white">Nov 1, 2024</td>
                    <td className="px-4 py-3 text-sm text-gray-600 dark:text-gray-400">{appName} - {currentPlan.name} Plan</td>
                    <td className="px-4 py-3 text-sm font-medium text-gray-900 dark:text-white">${currentPlan.price}.00</td>
                    <td className="px-4 py-3"><Badge variant="secondary" className="bg-green-100 text-green-700">Paid</Badge></td>
                    <td className="px-4 py-3 text-right"><Button variant="ghost" size="sm">PDF</Button></td>
                  </tr>
                  <tr>
                    <td className="px-4 py-3 text-sm text-gray-900 dark:text-white">Oct 1, 2024</td>
                    <td className="px-4 py-3 text-sm text-gray-600 dark:text-gray-400">{appName} - {currentPlan.name} Plan</td>
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
                      Are you sure you want to cancel the subscription for <strong>{appName}</strong>?
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
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Messages Today</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">2,847</div>
              </Card>
              <Card className="p-4">
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Avg Response</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">12ms</div>
              </Card>
              <Card className="p-4">
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Active Clients</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">142</div>
              </Card>
            </div>
          </Card>
        </TabsContent>

        {/* API Keys Tab */}
        <TabsContent value="keys">
          <Card className="p-6">
            <p className="text-gray-600 dark:text-gray-400">API keys management coming soon...</p>
          </Card>
        </TabsContent>

        {/* Webhooks Tab */}
        <TabsContent value="webhooks">
          <Card className="p-6">
            <p className="text-gray-600 dark:text-gray-400">Webhooks configuration coming soon...</p>
          </Card>
        </TabsContent>

        {/* Settings Tab */}
        <TabsContent value="settings">
          <Card className="p-6">
            <p className="text-gray-600 dark:text-gray-400">Settings coming soon...</p>
          </Card>
        </TabsContent>
      </Tabs>

      {/* Change Plan Dialog */}
      <Dialog open={upgradeDialogOpen} onOpenChange={setUpgradeDialogOpen}>
        <DialogContent className="max-w-3xl">
          <DialogHeader>
            <DialogTitle>Change Plan for {appName}</DialogTitle>
            <DialogDescription>
              Choose a plan that best fits your needs
            </DialogDescription>
          </DialogHeader>
          <div className="grid md:grid-cols-3 gap-6 py-6">
            {plans.map((plan) => (
              <div
                key={plan.name}
                className={`p-6 rounded-lg border-2 transition-all ${
                  plan.name === appTier
                    ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/10'
                    : 'border-gray-200 dark:border-gray-700'
                }`}
              >
                {plan.name === appTier && (
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
                    plan.name === appTier
                      ? 'bg-gray-100 text-gray-400 cursor-not-allowed dark:bg-gray-800'
                      : 'bg-blue-600 hover:bg-blue-700'
                  }`}
                  disabled={plan.name === appTier}
                  onClick={() => setUpgradeDialogOpen(false)}
                >
                  {plan.name === appTier ? 'Current Plan' : plan.price > currentPlan.price ? 'Upgrade' : 'Downgrade'}
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
