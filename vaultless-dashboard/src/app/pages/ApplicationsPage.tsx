import { Link } from 'react-router-dom';
import { motion } from 'motion/react';
import { Plus, Search, BarChart3, Key, Settings, Server } from 'lucide-react';
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
    createdAt: '2 months ago'
  },
  {
    id: '2',
    name: 'Staging Environment',
    description: 'Testing and staging deployment',
    tier: 'Free',
    status: 'active',
    quota: { used: 450, limit: 1000 },
    keys: 1,
    webhooks: 1,
    createdAt: '1 month ago'
  },
  {
    id: '3',
    name: 'Mobile App Backend',
    description: 'iOS and Android messaging service',
    tier: 'Pro',
    status: 'active',
    quota: { used: 42000, limit: 100000 },
    keys: 2,
    webhooks: 2,
    createdAt: '3 weeks ago'
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
            <Card className="p-6 hover:shadow-lg transition-shadow">
              <div className="flex items-start justify-between mb-4">
                <div className="flex-1">
                  <div className="flex items-center gap-2 mb-1">
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
                <Badge variant={app.tier === 'Pro' ? 'default' : 'secondary'}>
                  {app.tier}
                </Badge>
              </div>

              {/* Quota */}
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
                  {app.quota.used.toLocaleString()} / {app.quota.limit.toLocaleString()} messages
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
                    Manage
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
