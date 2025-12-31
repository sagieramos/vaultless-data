import { Link } from 'react-router-dom';
import { motion } from 'motion/react';
import { Plus, BarChart3, Key, Book, TrendingUp, TrendingDown, MessageSquare, Server, DollarSign, Zap } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import DashboardLayout from '../components/layout/DashboardLayout';
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';

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
  const hasApps = true; // Change to false to see empty state

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
            <h1 className="text-3xl font-bold text-gray-900 dark:text-white">Welcome back, Alex!</h1>
            <p className="text-gray-600 dark:text-gray-400 mt-1">
              {hasApps ? 'You have 3 active applications' : 'Ready to get started?'}
            </p>
          </div>
          <Link to="/applications/new">
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
              <Card className="p-6">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Total Messages</span>
                  <MessageSquare className="w-5 h-5 text-blue-600" />
                </div>
                <div className="flex items-end gap-2">
                  <span className="text-3xl font-bold text-gray-900 dark:text-white">65,342</span>
                  <span className="text-sm text-green-600 flex items-center mb-1">
                    <TrendingUp className="w-4 h-4 mr-1" />
                    12%
                  </span>
                </div>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">This month</p>
              </Card>
            </motion.div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.2 }}
            >
              <Card className="p-6">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Active Applications</span>
                  <Server className="w-5 h-5 text-purple-600" />
                </div>
                <div className="flex items-end gap-2">
                  <span className="text-3xl font-bold text-gray-900 dark:text-white">3</span>
                  <span className="text-sm text-gray-600 flex items-center mb-1">
                    <Zap className="w-4 h-4 mr-1" />
                    All active
                  </span>
                </div>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">Running smoothly</p>
              </Card>
            </motion.div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.3 }}
            >
              <Card className="p-6">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Quota Used</span>
                  <BarChart3 className="w-5 h-5 text-green-600" />
                </div>
                <div className="flex items-end gap-2">
                  <span className="text-3xl font-bold text-gray-900 dark:text-white">65%</span>
                  <span className="text-sm text-gray-600 flex items-center mb-1">
                    of 100K
                  </span>
                </div>
                <div className="mt-2 h-2 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                  <div className="h-full bg-green-600" style={{ width: '65%' }} />
                </div>
              </Card>
            </motion.div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.4 }}
            >
              <Card className="p-6">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm font-medium text-gray-600 dark:text-gray-400">Total Cost</span>
                  <DollarSign className="w-5 h-5 text-orange-600" />
                </div>
                <div className="flex items-end gap-2">
                  <span className="text-3xl font-bold text-gray-900 dark:text-white">$45.23</span>
                  <span className="text-sm text-green-600 flex items-center mb-1">
                    <TrendingDown className="w-4 h-4 mr-1" />
                    8%
                  </span>
                </div>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">This month</p>
              </Card>
            </motion.div>
          </div>

          {/* Charts and Activity Row */}
          <div className="grid lg:grid-cols-3 gap-6 mb-8">
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.5 }}
              className="lg:col-span-2"
            >
              <Card className="p-6">
                <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">Message Volume</h2>
                <ResponsiveContainer width="100%" height={300}>
                  <LineChart data={data}>
                    <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                    <XAxis dataKey="name" className="text-gray-600 dark:text-gray-400" />
                    <YAxis className="text-gray-600 dark:text-gray-400" />
                    <Tooltip
                      contentStyle={{
                        backgroundColor: 'var(--background)',
                        border: '1px solid var(--border)',
                        borderRadius: '8px'
                      }}
                    />
                    <Line type="monotone" dataKey="messages" stroke="#2563eb" strokeWidth={2} />
                  </LineChart>
                </ResponsiveContainer>
              </Card>
            </motion.div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.6 }}
            >
              <Card className="p-6">
                <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">Recent Activity</h2>
                <div className="space-y-4">
                  {recentActivity.map((activity, index) => (
                    <div key={index} className="flex items-start gap-3">
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
            transition={{ delay: 0.7 }}
          >
            <h2 className="text-xl font-semibold mb-6 text-gray-900 dark:text-white">Quick Actions</h2>
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
              {quickActions.map((action, index) => (
                <Link key={action.title} to={action.link}>
                  <Card className="p-6 hover:shadow-lg transition-all cursor-pointer group">
                    <div className={`w-12 h-12 rounded-lg ${action.color} flex items-center justify-center mb-4 group-hover:scale-110 transition-transform`}>
                      <action.icon className="w-6 h-6 text-white" />
                    </div>
                    <h3 className="font-semibold text-gray-900 dark:text-white mb-1">{action.title}</h3>
                    <p className="text-sm text-gray-600 dark:text-gray-400">{action.description}</p>
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
            <Link to="/applications/new">
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
