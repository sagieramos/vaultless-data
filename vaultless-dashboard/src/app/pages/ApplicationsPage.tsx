import { Link } from 'react-router-dom';
import { motion } from 'motion/react';
import { Plus, Search, BarChart3, Key, Settings, Server, Cpu, ShieldCheck, Wifi, MessageSquare } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Input } from '../components/ui/input';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Progress } from '../components/ui/progress';
import DashboardLayout from '../components/layout/DashboardLayout';

const applications = [
  {
    id: '1',
    name: 'Production API',
    description: 'Main production messaging service',
    tier: 'Pro',
    status: 'active',
    quota: { used: 65000, limit: 100000 },
    keys: 2,
    webhooks: 3,
    createdAt: '2 months ago',
    type: 'messaging',
    icon: MessageSquare
  },
  {
    id: '2',
    name: 'Staging Environment',
    description: 'Testing and staging deployment',
    tier: 'Free',
    status: 'active',
    quota: { used: 450, limit: 10000 },
    keys: 1,
    webhooks: 1,
    createdAt: '1 month ago',
    type: 'messaging',
    icon: MessageSquare
  },
  {
    id: '3',
    name: 'Smart Home Hub',
    description: 'IoT device management and attestation',
    tier: 'Enterprise',
    status: 'active',
    quota: { used: 12500, limit: 500000 },
    keys: 3,
    webhooks: 5,
    createdAt: '3 weeks ago',
    type: 'iot',
    iotStats: {
      devices: 847,
      attested: 842,
      trusted: 98
    }
  },
  {
    id: '4',
    name: 'Mobile App Backend',
    description: 'iOS and Android messaging service',
    tier: 'Pro',
    status: 'active',
    quota: { used: 42000, limit: 100000 },
    keys: 2,
    webhooks: 2,
    createdAt: '3 weeks ago',
    type: 'messaging',
    icon: MessageSquare
  },
];

export default function ApplicationsPage() {
  return (
    <DashboardLayout>
      <div className="mb-8 flex flex-col md:flex-row md:items-center md:justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white">Applications</h1>
          <p className="text-gray-600 dark:text-gray-400 mt-1">
            Manage your applications and API integrations
          </p>
        </div>
        <Link to="/applications/new">
          <Button className="bg-blue-600 hover:bg-blue-700">
            <Plus className="w-4 h-4 mr-2" />
            Create App
          </Button>
        </Link>
      </div>

      {/* Search and Filters */}
      <div className="mb-6 flex gap-4">
        <div className="flex-1 relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-5 h-5 text-gray-400" />
          <Input placeholder="Search applications..." className="pl-10" />
        </div>
        <Button variant="outline">Filter</Button>
        <Button variant="outline">Sort</Button>
      </div>

      {/* Applications Grid */}
      <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-6">
        {applications.map((app, index) => (
          <motion.div
            key={app.id}
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: index * 0.1 }}
          >
            <Card className={`p-6 hover:shadow-lg transition-shadow ${app.type === 'iot' ? 'border-purple-200 dark:border-purple-800 bg-purple-50/50 dark:bg-purple-900/10' : ''}`}>
              <div className="flex items-start justify-between mb-4">
                <div className="flex-1">
                  <div className="flex items-center gap-2 mb-1">
                    {app.type === 'iot' ? (
                      <div className="w-8 h-8 bg-purple-100 dark:bg-purple-900/30 rounded-lg flex items-center justify-center">
                        <Cpu className="w-4 h-4 text-purple-600" />
                      </div>
                    ) : (
                      <div className="w-8 h-8 bg-blue-100 dark:bg-blue-900/30 rounded-lg flex items-center justify-center">
                        <MessageSquare className="w-4 h-4 text-blue-600" />
                      </div>
                    )}
                    <h3 className="font-semibold text-lg text-gray-900 dark:text-white">
                      {app.name}
                    </h3>
                    {app.status === 'active' && (
                      <div className="w-2 h-2 rounded-full bg-green-500" />
                    )}
                  </div>
                  <p className="text-sm text-gray-600 dark:text-gray-400 line-clamp-1">
                    {app.description}
                  </p>
                </div>
                <div className="flex flex-col items-end gap-1">
                  <Badge variant={app.tier === 'Pro' || app.tier === 'Enterprise' ? 'default' : 'secondary'}>
                    {app.tier}
                  </Badge>
                  {app.type === 'iot' && (
                    <Badge variant="outline" className="text-xs text-purple-600 border-purple-300">
                      <Wifi className="w-3 h-3 mr-1" />
                      IoT
                    </Badge>
                  )}
                </div>
              </div>

              {/* Quota (for all apps) */}
              <div className="mb-4">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm text-gray-600 dark:text-gray-400">Quota</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white">
                    {Math.round((app.quota.used / app.quota.limit) * 100)}%
                  </span>
                </div>
                <Progress
                  value={(app.quota.used / app.quota.limit) * 100}
                  className="h-2"
                />
                <p className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                  {app.quota.used.toLocaleString()} / {app.quota.limit >= 999999 ? 'Unlimited' : app.quota.limit.toLocaleString()} messages
                </p>
              </div>

              {/* Stats */}
              <div className="flex items-center gap-4 mb-4 text-sm text-gray-600 dark:text-gray-400">
                <span className="flex items-center gap-1">
                  <Key className="w-4 h-4" /> {app.keys} keys
                </span>
                <span className="flex items-center gap-1">
                  <Server className="w-4 h-4" /> {app.webhooks} webhooks
                </span>
              </div>

              <p className="text-xs text-gray-600 dark:text-gray-400 mb-4">
                Created {app.createdAt}
              </p>

              {/* Actions */}
              <div className="flex gap-2">
                <Link to={`/applications/${app.id}`} className="flex-1">
                  <Button variant="outline" size="sm" className="w-full">
                    <Settings className="w-4 h-4 mr-2" />
                    {app.type === 'iot' ? 'Devices' : 'Manage'}
                  </Button>
                </Link>
                <Link to={`/applications/${app.id}/analytics`}>
                  <Button variant="outline" size="sm">
                    <BarChart3 className="w-4 h-4" />
                  </Button>
                </Link>
              </div>
            </Card>
          </motion.div>
        ))}
      </div>

      {/* Pagination */}
      <div className="mt-8 flex items-center justify-between">
        <p className="text-sm text-gray-600 dark:text-gray-400">
          Showing 1-3 of 3 applications
        </p>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" disabled>
            Previous
          </Button>
          <Button variant="outline" size="sm" disabled>
            Next
          </Button>
        </div>
      </div>
    </DashboardLayout>
  );
}
