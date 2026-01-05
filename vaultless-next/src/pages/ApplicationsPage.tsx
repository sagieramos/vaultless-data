"use client";
import { useState, useEffect } from 'react';
import Link from 'next/link';
import { motion } from 'motion/react';
import { Plus, Search, BarChart3, Key, Settings, Server, Cpu, ShieldCheck, Wifi, MessageSquare, Loader2 } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Input } from '../components/ui/input';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Progress } from '../components/ui/progress';
import DashboardLayout from '../components/layout/DashboardLayout';
import { useRequireAuth } from '@/contexts/AuthContext';
import { applicationsApi } from '@/lib/api';

export default function ApplicationsPage() {
  useRequireAuth();
  const [applications, setApplications] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    applicationsApi.list()
      .then(res => {
        setApplications(res.data || []);
        setLoading(false);
      })
      .catch(err => {
        console.error('Failed to fetch applications:', err);
        setError(err.message || 'Failed to load applications');
        setLoading(false);
      });
  }, []);
  return (
    <DashboardLayout>
      <div className="mb-8 flex flex-col md:flex-row md:items-center md:justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white">Applications</h1>
          <p className="text-gray-600 dark:text-gray-400 mt-1">
            Manage your applications and API integrations
          </p>
        </div>
        <Link href="/applications/new">
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

      {/* Loading State */}
      {loading && (
        <div className="flex items-center justify-center py-12">
          <Loader2 className="w-8 h-8 animate-spin text-blue-600" />
          <span className="ml-3 text-gray-600 dark:text-gray-400">Loading applications...</span>
        </div>
      )}

      {/* Error State */}
      {error && (
        <Card className="p-6 border-red-200 bg-red-50 dark:bg-red-900/10">
          <p className="text-red-600 dark:text-red-400">{error}</p>
        </Card>
      )}

      {/* Empty State */}
      {!loading && !error && applications.length === 0 && (
        <Card className="p-12 text-center">
          <Server className="w-16 h-16 mx-auto text-gray-400 mb-4" />
          <h3 className="text-xl font-semibold text-gray-900 dark:text-white mb-2">No Applications Yet</h3>
          <p className="text-gray-600 dark:text-gray-400 mb-6">
            Create your first application to start using Vaultless
          </p>
          <Link href="/applications/new">
            <Button>
              <Plus className="w-4 h-4 mr-2" />
              Create Application
            </Button>
          </Link>
        </Card>
      )}

      {/* Applications Grid */}
      {!loading && !error && applications.length > 0 && (
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
                    <div className="w-8 h-8 bg-blue-100 dark:bg-blue-900/30 rounded-lg flex items-center justify-center">
                      <MessageSquare className="w-4 h-4 text-blue-600" />
                    </div>
                    <h3 className="font-semibold text-lg text-gray-900 dark:text-white">
                      {app.name}
                    </h3>
                    {app.is_active && (
                      <div className="w-2 h-2 rounded-full bg-green-500" />
                    )}
                  </div>
                  <p className="text-sm text-gray-600 dark:text-gray-400 line-clamp-1">
                    {app.description || 'No description'}
                  </p>
                </div>
                <div className="flex flex-col items-end gap-1">
                  <Badge variant={app.tier === 'pro' || app.tier === 'enterprise' ? 'default' : 'secondary'}>
                    {app.tier || 'Free'}
                  </Badge>
                </div>
              </div>

              <p className="text-xs text-gray-600 dark:text-gray-400 mb-4">
                Created {new Date(app.created_at).toLocaleDateString()}
              </p>

              {/* Actions */}
              <div className="flex gap-2">
                <Link href={`/applications/${app.id}`} className="flex-1">
                  <Button variant="outline" size="sm" className="w-full">
                    <Settings className="w-4 h-4 mr-2" />
                    Manage
                  </Button>
                </Link>
                <Link href={`/applications/${app.id}/analytics`}>
                  <Button variant="outline" size="sm">
                    <BarChart3 className="w-4 h-4" />
                  </Button>
                </Link>
              </div>
            </Card>
          </motion.div>
        ))}
      </div>
      )}

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
