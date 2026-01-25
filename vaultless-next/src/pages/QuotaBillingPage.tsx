"use client";
import { motion } from 'motion/react';
import { useState } from 'react';
import {
  CreditCard, Download, Zap, Edit, Calendar, DollarSign, MessageSquare, Activity
} from 'lucide-react';

import Link from 'next/link';
import DashboardLayout from '../components/layout/DashboardLayout';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Progress } from '../components/ui/progress';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs';

// Application billing summaries
const applicationBilling = [
  {
    id: '1',
    name: 'Production API',
    plan: 'Pro',
    price: 29,
    status: 'active',
    usage: { messages: 65000, bandwidth: 32.4, storage: 4.2 },
    limits: { messages: 100000, bandwidth: 50, storage: 10 }
  },
  {
    id: '2',
    name: 'Staging Environment',
    plan: 'Free',
    price: 0,
    status: 'active',
    usage: { messages: 450, bandwidth: 0.5, storage: 0.1 },
    limits: { messages: 10000, bandwidth: 5, storage: 1 }
  },
  {
    id: '3',
    name: 'Smart Home Hub',
    plan: 'Enterprise',
    price: 99,
    status: 'active',
    usage: { messages: 12500, bandwidth: 156, storage: 12 },
    limits: { messages: 999999999, bandwidth: 500, storage: 100 }
  },
  {
    id: '4',
    name: 'Mobile App Backend',
    plan: 'Pro',
    price: 29,
    status: 'active',
    usage: { messages: 42000, bandwidth: 18.2, storage: 3.1 },
    limits: { messages: 100000, bandwidth: 50, storage: 10 }
  }
];

// Billing history
const billingHistory = [
  { id: '1', date: 'Dec 1, 2024', description: 'Production API - Pro Plan', amount: 29.00, status: 'paid' },
  { id: '2', date: 'Dec 1, 2024', description: 'Smart Home Hub - Enterprise Plan', amount: 99.00, status: 'paid' },
  { id: '3', date: 'Dec 1, 2024', description: 'Mobile App Backend - Pro Plan', amount: 29.00, status: 'paid' },
  { id: '4', date: 'Nov 1, 2024', description: 'Production API - Pro Plan', amount: 29.00, status: 'paid' },
  { id: '5', date: 'Nov 1, 2024', description: 'Mobile App Backend - Pro Plan', amount: 29.00, status: 'paid' },
];

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
    features: ['Unlimited messages', '500 GB bandwidth', '100 GB storage', 'Unlimited API keys', '24/7 support'],
  }
];

// Payment methods
const paymentMethods = [
  { id: '1', type: 'card', brand: 'Visa', last4: '4242', expiry: '12/26', isDefault: true },
];

export default function QuotaBillingPage() {
  const [activeTab, setActiveTab] = useState('overview');

  // Application billing summaries
  const applicationBilling = [
    {
      id: '1',
      name: 'Production API',
      plan: 'Pro',
      price: 29,
      status: 'active',
      usage: { messages: 65000, bandwidth: 32.4, storage: 4.2 },
      limits: { messages: 100000, bandwidth: 50, storage: 10 }
    },
    {
      id: '2',
      name: 'Staging Environment',
      plan: 'Free',
      price: 0,
      status: 'active',
      usage: { messages: 450, bandwidth: 0.5, storage: 0.1 },
      limits: { messages: 10000, bandwidth: 5, storage: 1 }
    },
    {
      id: '3',
      name: 'Smart Home Hub',
      plan: 'Enterprise',
      price: 99,
      status: 'active',
      usage: { messages: 12500, bandwidth: 156, storage: 12 },
      limits: { messages: 999999999, bandwidth: 500, storage: 100 }
    },
    {
      id: '4',
      name: 'Mobile App Backend',
      plan: 'Pro',
      price: 29,
      status: 'active',
      usage: { messages: 42000, bandwidth: 18.2, storage: 3.1 },
      limits: { messages: 100000, bandwidth: 50, storage: 10 }
    }
  ];

  // Billing history
  const billingHistory = [
    { id: '1', date: 'Dec 1, 2024', description: 'Production API - Pro Plan', amount: 29.00, status: 'paid' },
    { id: '2', date: 'Dec 1, 2024', description: 'Smart Home Hub - Enterprise Plan', amount: 99.00, status: 'paid' },
    { id: '3', date: 'Dec 1, 2024', description: 'Mobile App Backend - Pro Plan', amount: 29.00, status: 'paid' },
    { id: '4', date: 'Nov 1, 2024', description: 'Production API - Pro Plan', amount: 29.00, status: 'paid' },
    { id: '5', date: 'Nov 1, 2024', description: 'Mobile App Backend - Pro Plan', amount: 29.00, status: 'paid' },
  ];

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
      features: ['Unlimited messages', '500 GB bandwidth', '100 GB storage', 'Unlimited API keys', '24/7 support'],
    }
  ];

  // Payment methods
  const paymentMethods = [
    { id: '1', type: 'card', brand: 'Visa', last4: '4242', expiry: '12/26', isDefault: true },
  ];

  const totalMonthly = applicationBilling.reduce((sum, app) => sum + app.price, 0);
  const totalApps = applicationBilling.length;
  const totalMessages = applicationBilling.reduce((sum, app) => sum + app.usage.messages, 0);
  const totalBandwidth = applicationBilling.reduce((sum, app) => sum + app.usage.bandwidth, 0);

  const formatNumber = (num: number) => {
    if (num >= 1000000) return (num / 1000000).toFixed(1) + 'M';
    if (num >= 1000) return (num / 1000).toFixed(1) + 'K';
    return num.toString();
  };

  const calculatePercentage = (used: number, total: number) => {
    if (total >= 999999999) return 0; // Unlimited
    return Math.min((used / total) * 100, 100);
  };

  return (
    <DashboardLayout>
      {/* Header */}
      <div className="mb-6">
        <div className="flex items-start justify-between">
          <div>
            <h1 className="text-3xl font-bold text-gray-900 dark:text-white">Quota & Billing</h1>
            <p className="text-gray-600 dark:text-gray-400">
              Manage billing for each application individually
            </p>
          </div>
          <div className="flex gap-2">
            <Button variant="outline">
              <Download className="w-4 h-4 mr-2" />
              Download All Invoices
            </Button>
          </div>
        </div>
      </div>

      {/* Summary Stats */}
      <div className="grid md:grid-cols-4 gap-6 mb-8">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Monthly Total</span>
              <DollarSign className="w-5 h-5 text-green-600" />
            </div>
            <div className="text-3xl font-bold text-gray-900 dark:text-white">
              ${totalMonthly}
            </div>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">Across {totalApps} applications</p>
          </Card>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.2 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Total Messages</span>
              <MessageSquare className="w-5 h-5 text-blue-600" />
            </div>
            <div className="text-3xl font-bold text-gray-900 dark:text-white">
              {formatNumber(totalMessages)}
            </div>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">This month</p>
          </Card>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.3 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Bandwidth</span>
              <Activity className="w-5 h-5 text-purple-600" />
            </div>
            <div className="text-3xl font-bold text-gray-900 dark:text-white">
              {totalBandwidth.toFixed(1)} GB
            </div>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">This month</p>
          </Card>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.4 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Next Billing</span>
              <Calendar className="w-5 h-5 text-orange-600" />
            </div>
            <div className="text-3xl font-bold text-gray-900 dark:text-white">
              Jan 1
            </div>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">in 15 days</p>
          </Card>
        </motion.div>
      </div>

      <Tabs defaultValue="overview" className="space-y-6" onValueChange={setActiveTab}>
        <TabsList>
          <TabsTrigger value="overview">Applications</TabsTrigger>
          <TabsTrigger value="billing">Billing History</TabsTrigger>
          <TabsTrigger value="payment">Payment Methods</TabsTrigger>
        </TabsList>

        {/* Applications Overview Tab */}
        <TabsContent value="overview" className="space-y-6">
          <div className="grid gap-6">
            {applicationBilling.map((app) => (
              <Card key={app.id} className="p-6">
                <div className="flex items-center justify-between mb-4">
                  <div className="flex items-center gap-4">
                    <div className={`w-16 h-16 ${app.plan === 'Enterprise'
                      ? 'bg-gradient-to-br from-purple-500 to-purple-600'
                      : app.plan === 'Pro'
                        ? 'bg-gradient-to-br from-blue-500 to-blue-600'
                        : 'bg-gray-100 dark:bg-gray-800'
                      } rounded-xl flex items-center justify-center`}>
                      <Zap className={`w-8 h-8 ${app.plan === 'Free' ? 'text-gray-400' : 'text-white'
                        }`} />
                    </div>
                    <div>
                      <div className="flex items-center gap-3 mb-1">
                        <h3 className="text-xl font-bold text-gray-900 dark:text-white">{app.name}</h3>
                        <Badge>{app.plan}</Badge>
                      </div>
                      <p className="text-gray-600 dark:text-gray-400">
                        ${app.price}/month • Renews on Jan 1, 2025
                      </p>
                      <div className="flex items-center gap-2 mt-1">
                        <div className="w-2 h-2 rounded-full bg-green-500"></div>
                        <span className="text-sm text-gray-600 dark:text-gray-400 capitalize">{app.status}</span>
                      </div>
                    </div>
                  </div>
                  <Link href={`/applications/${app.id}`}>
                    <Button variant="outline">
                      <Edit className="w-4 h-4 mr-2" />
                      Edit
                    </Button>
                  </Link>
                </div>

                <div className="mb-6">
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-sm text-gray-600 dark:text-gray-400">Messages</span>
                    <span className="text-sm font-medium text-gray-900 dark:text-white">
                      {formatNumber(app.usage.messages)} / {app.limits.messages >= 999999999 ? 'Unlimited' : formatNumber(app.limits.messages)}
                    </span>
                  </div>
                  <Progress value={calculatePercentage(app.usage.messages, app.limits.messages)} className="h-3" />
                  <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                    {app.plan === 'Enterprise' ? 'Unlimited' : app.plan === 'Pro' ? '100K' : '10K'} messages/month
                  </p>
                </div>

                <div className="grid md:grid-cols-3 gap-6">
                  <div>
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-sm text-gray-600 dark:text-gray-400">Bandwidth</span>
                      <span className="text-sm font-medium text-gray-900 dark:text-white">
                        {app.usage.bandwidth} / {app.limits.bandwidth} GB
                      </span>
                    </div>
                    <Progress value={calculatePercentage(app.usage.bandwidth, app.limits.bandwidth)} className="h-3" />
                    <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                      of {app.limits.bandwidth} GB
                    </p>
                  </div>
                  <div>
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-sm text-gray-600 dark:text-gray-400">Storage</span>
                      <span className="text-sm font-medium text-gray-900 dark:text-white">
                        {app.usage.storage} / {app.limits.storage} GB
                      </span>
                    </div>
                    <Progress value={calculatePercentage(app.usage.storage, app.limits.storage)} className="h-3" />
                    <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                      of {app.limits.storage} GB
                    </p>
                  </div>
                  <div>
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-sm text-gray-600 dark:text-gray-400">Plan Cost</span>
                      <span className="text-sm font-medium text-gray-900 dark:text-white">${app.price}/mo</span>
                    </div>
                    <Progress value={app.plan === 'Free' ? 0 : 100} className="h-3" />
                    <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                      {app.plan === 'Free' ? 'Free tier' : 'Current plan'}
                    </p>
                  </div>
                </div>
              </Card>
            ))}
          </div>
        </TabsContent>

        {/* Billing History Tab */}
        <TabsContent value="billing">
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
                  {billingHistory.map((invoice) => (
                    <tr key={invoice.id}>
                      <td className="px-4 py-3 text-sm text-gray-900 dark:text-white">{invoice.date}</td>
                      <td className="px-4 py-3 text-sm text-gray-600 dark:text-gray-400">{invoice.description}</td>
                      <td className="px-4 py-3 text-sm font-medium text-gray-900 dark:text-white">${invoice.amount.toFixed(2)}</td>
                      <td className="px-4 py-3"><Badge variant="secondary" className="bg-green-100 text-green-700">Paid</Badge></td>
                      <td className="px-4 py-3 text-right"><Button variant="ghost" size="sm">PDF</Button></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </Card>

          {/* Monthly Summary */}
          <Card className="p-6 mt-6">
            <h3 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">December 2024 Summary</h3>
            <div className="grid md:grid-cols-4 gap-6">
              <div>
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Production API</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">$29.00</div>
              </div>
              <div>
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Smart Home Hub</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">$99.00</div>
              </div>
              <div>
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Mobile App Backend</div>
                <div className="text-2xl font-bold text-gray-900 dark:text-white">$29.00</div>
              </div>
              <div>
                <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Total</div>
                <div className="text-3xl font-bold text-gray-900 dark:text-white">$157.00</div>
              </div>
            </div>
          </Card>
        </TabsContent>

        {/* Payment Methods Tab */}
        <TabsContent value="payment" className="space-y-6">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <div>
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Payment Methods</h2>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                  Manage your payment methods for all applications
                </p>
              </div>
              <Button>
                <CreditCard className="w-4 h-4 mr-2" />
                Add Payment Method
              </Button>
            </div>

            <div className="space-y-4">
              {paymentMethods.map((method) => (
                <Card key={method.id} className="p-4">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-4">
                      <div className="w-12 h-8 bg-gray-100 dark:bg-gray-800 rounded flex items-center justify-center">
                        <CreditCard className="w-5 h-5 text-gray-600 dark:text-gray-400" />
                      </div>
                      <div>
                        <div className="flex items-center gap-2">
                          <span className="font-medium text-gray-900 dark:text-white">{method.brand}</span>
                          <span className="text-gray-600 dark:text-gray-400">•••• {method.last4}</span>
                          {method.isDefault && (
                            <Badge variant="secondary" className="text-xs">Default</Badge>
                          )}
                        </div>
                        <p className="text-sm text-gray-600 dark:text-gray-400">Expires {method.expiry}</p>
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      {!method.isDefault && (
                        <Button variant="ghost" size="sm">Set as Default</Button>
                      )}
                      <Button variant="ghost" size="sm" className="text-red-600 hover:text-red-700">Remove</Button>
                    </div>
                  </div>
                </Card>
              ))}
            </div>
          </Card>

          {/* Billing Address */}
          <Card className="p-6">
            <div className="flex items-center justify-between mb-6">
              <div>
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Billing Address</h2>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                  Your billing information for invoices
                </p>
              </div>
              <Button variant="outline">Edit</Button>
            </div>
            <div className="text-gray-900 dark:text-white">
              <p className="font-medium">Vaultless Inc.</p>
              <p className="text-gray-600 dark:text-gray-400">123 Tech Street</p>
              <p className="text-gray-600 dark:text-gray-400">San Francisco, CA 94105</p>
              <p className="text-gray-600 dark:text-gray-400">United States</p>
            </div>
          </Card>
        </TabsContent>
      </Tabs>
    </DashboardLayout>
  );
}
