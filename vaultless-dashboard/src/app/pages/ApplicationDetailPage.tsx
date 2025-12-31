import { Link, useParams } from 'react-router-dom';
import { ArrowLeft, Edit, BarChart3, Key, Webhook, Settings as SettingsIcon } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Progress } from '../components/ui/progress';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs';
import DashboardLayout from '../components/layout/DashboardLayout';

export default function ApplicationDetailPage() {
  const { id } = useParams();

  return (
    <DashboardLayout>
      <div className="mb-6">
        <Link to="/applications" className="inline-flex items-center text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white mb-4">
          <ArrowLeft className="w-4 h-4 mr-2" />
          Back to Apps
        </Link>
        
        <div className="flex items-start justify-between">
          <div>
            <div className="flex items-center gap-3 mb-2">
              <h1 className="text-3xl font-bold text-gray-900 dark:text-white">Production API</h1>
              <Badge>Pro</Badge>
              <div className="flex items-center gap-2">
                <div className="w-2 h-2 rounded-full bg-green-500" />
                <span className="text-sm text-gray-600 dark:text-gray-400">Active</span>
              </div>
            </div>
            <p className="text-gray-600 dark:text-gray-400">Main production messaging service</p>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">Created 2 months ago</p>
          </div>
          
          <Button variant="outline">
            <Edit className="w-4 h-4 mr-2" />
            Edit
          </Button>
        </div>
      </div>

      <Tabs defaultValue="overview" className="space-y-6">
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="analytics">Analytics</TabsTrigger>
          <TabsTrigger value="keys">API Keys</TabsTrigger>
          <TabsTrigger value="webhooks">Webhooks</TabsTrigger>
          <TabsTrigger value="settings">Settings</TabsTrigger>
        </TabsList>

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
              <div className="text-sm text-gray-600 dark:text-gray-400 mt-1">of 50 GB</div>
            </Card>
            
            <Card className="p-6">
              <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Active Clients</div>
              <div className="text-3xl font-bold text-gray-900 dark:text-white">142</div>
              <div className="text-sm text-green-600 mt-1">+8 new today</div>
            </Card>
            
            <Card className="p-6">
              <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Cost</div>
              <div className="text-3xl font-bold text-gray-900 dark:text-white">$29</div>
              <div className="text-sm text-gray-600 dark:text-gray-400 mt-1">This month</div>
            </Card>
          </div>

          <Card className="p-6">
            <h2 className="text-xl font-semibold mb-4 text-gray-900 dark:text-white">Quota Usage</h2>
            <div className="mb-4">
              <div className="flex items-center justify-between mb-2">
                <span className="text-sm text-gray-600 dark:text-gray-400">Messages</span>
                <span className="text-sm font-medium text-gray-900 dark:text-white">65%</span>
              </div>
              <Progress value={65} className="h-3" />
              <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                65,000 / 100,000 messages • Resets in 15 days
              </p>
            </div>
            
            {65 > 80 && (
              <div className="mt-4 p-4 bg-yellow-50 dark:bg-yellow-900/20 rounded-lg">
                <p className="text-sm text-yellow-800 dark:text-yellow-200">
                  You're using over 80% of your quota. Consider upgrading to avoid interruptions.
                </p>
                <Button size="sm" className="mt-2">Upgrade Plan</Button>
              </div>
            )}
          </Card>
        </TabsContent>

        <TabsContent value="analytics">
          <Card className="p-6">
            <p className="text-gray-600 dark:text-gray-400">Analytics view coming soon...</p>
          </Card>
        </TabsContent>

        <TabsContent value="keys">
          <Card className="p-6">
            <p className="text-gray-600 dark:text-gray-400">API keys management coming soon...</p>
          </Card>
        </TabsContent>

        <TabsContent value="webhooks">
          <Card className="p-6">
            <p className="text-gray-600 dark:text-gray-400">Webhooks configuration coming soon...</p>
          </Card>
        </TabsContent>

        <TabsContent value="settings">
          <Card className="p-6">
            <p className="text-gray-600 dark:text-gray-400">Settings coming soon...</p>
          </Card>
        </TabsContent>
      </Tabs>
    </DashboardLayout>
  );
}
