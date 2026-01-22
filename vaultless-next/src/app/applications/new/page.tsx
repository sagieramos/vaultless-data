"use client";

export const dynamic = 'force-dynamic';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { motion } from 'motion/react';
import { Copy, Check, Eye, EyeOff, AlertTriangle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Card } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import { toast } from 'sonner';
import DashboardLayout from '@/components/layout/DashboardLayout';
import { useRequireAuth } from '@/contexts/AuthContext';
import { applicationsApi } from '@/lib/api';

export default function CreateAppPage() {
  const router = useRouter();
  const queryClient = useQueryClient();
  const { isAuthenticated, isLoading: authLoading } = useRequireAuth();

  const [step, setStep] = useState(1);
  const [formData, setFormData] = useState({
    name: '',
    description: ''
  });
  const [keys, setKeys] = useState({ secret: '', publishable: '' });
  const [createdAppId, setCreatedAppId] = useState<string | null>(null);
  const [showSecret, setShowSecret] = useState(false);
  const [copied, setCopied] = useState({ secret: false, publishable: false });
  const [savedConfirmation, setSavedConfirmation] = useState(false);

  const createAppMutation = useMutation({
    mutationFn: (data: { name: string; description?: string }) => applicationsApi.create(data),
    onSuccess: (res) => {
      queryClient.invalidateQueries({ queryKey: ['applications'] });
      setKeys({ secret: res.secretKey || '', publishable: res.publishableKey || '' });
      setCreatedAppId(res.application?.id || null);
      setSavedConfirmation(false);
      toast.success(res.message || 'Application created successfully!');
      setStep(2);
    },
    onError: (err: any) => {
      console.error('Create application failed:', err);
      toast.error(err?.message || 'Failed to create application');
    },
  });

  if (authLoading) {
    return (
      <DashboardLayout>
        <div className="flex items-center justify-center min-h-screen">
          <div>Loading...</div>
        </div>
      </DashboardLayout>
    );
  }

  if (!isAuthenticated) {
    router.push('/login');
    return null;
  }

  const handleCopy = (text: string, type: 'secret' | 'publishable') => {
    navigator.clipboard.writeText(text);
    setCopied({ ...copied, [type]: true });
    toast.success(`${type === 'secret' ? 'Secret' : 'Publishable'} key copied!`);
    setTimeout(() => setCopied({ ...copied, [type]: false }), 2000);
  };

  const handleContinue = () => {
    if (!savedConfirmation) return;
    if (createdAppId) {
      router.push(`/applications/${createdAppId}`);
    } else {
      router.push('/applications');
    }
  };

  if (step === 2) {
    return (
      <DashboardLayout>
        <div className="max-w-3xl mx-auto">
          <motion.div
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
          >
            <Card className="p-8">
              <div className="text-center mb-8">
                <div className="w-16 h-16 bg-green-100 dark:bg-green-900/20 rounded-full flex items-center justify-center mx-auto mb-4">
                  <Check className="w-8 h-8 text-green-600" />
                </div>
                <h1 className="text-3xl font-bold text-gray-900 dark:text-white mb-2">
                  Application Created!
                </h1>
                <p className="text-gray-600 dark:text-gray-400">
                  Your application &quot;{formData.name}&quot; is ready
                </p>
              </div>

              <div className="bg-yellow-50 dark:bg-yellow-900/20 border-2 border-yellow-200 dark:border-yellow-800 rounded-lg p-4 mb-6">
                <div className="flex items-center gap-3">
                  <AlertTriangle className="w-6 h-6 text-yellow-600 flex-shrink-0 mt-0.5" />
                  <div>
                    <p className="font-semibold text-yellow-900 dark:text-yellow-100 mb-1">
                      Save your secret key now!
                    </p>
                    <p className="text-sm text-yellow-800 dark:text-yellow-200">
                      You won&apos;t be able to see it again.
                    </p>
                  </div>
                </div>
              </div>

              <div className="space-y-6 mb-8">
                <div className="border-2 border-red-200 dark:border-red-800 rounded-lg p-6 bg-red-50 dark:bg-red-900/10">
                  <div className="flex items-center justify-between mb-3">
                    <Label className="text-base font-semibold text-red-900 dark:text-red-100">
                      SECRET KEY
                    </Label>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setShowSecret(!showSecret)}
                    >
                      {showSecret ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
                    </Button>
                  </div>
                  <div className="flex items-center gap-2">
                    <Input
                      readOnly
                      value={showSecret ? keys.secret : '••••••••••••••••••••••••••••••••••••••••'}
                      className="font-mono text-sm bg-white dark:bg-gray-900"
                    />
                    <Button
                      variant="outline"
                      size="icon"
                      onClick={() => handleCopy(keys.secret, 'secret')}
                      disabled={!keys.secret}
                    >
                      {copied.secret ? <Check className="w-4 h-4 text-green-600" /> : <Copy className="w-4 h-4" />}
                    </Button>
                  </div>
                  {copied.secret && (
                    <p className="text-sm text-green-600 mt-2 flex items-center gap-1">
                      <Check className="w-4 h-4" /> Copied!
                    </p>
                  )}
                </div>

                <div className="border border-gray-200 dark:border-gray-700 rounded-lg p-6">
                  <Label className="text-base font-semibold mb-3 block">
                    PUBLISHABLE KEY
                  </Label>
                  <div className="flex items-center gap-2">
                    <Input
                      readOnly
                      value={keys.publishable}
                      className="font-mono text-sm"
                    />
                    <Button
                      variant="outline"
                      size="icon"
                      onClick={() => handleCopy(keys.publishable, 'publishable')}
                      disabled={!keys.publishable}
                    >
                      {copied.publishable ? <Check className="w-4 h-4 text-green-600" /> : <Copy className="w-4 h-4" />}
                    </Button>
                  </div>
                  {copied.publishable && (
                    <p className="text-sm text-green-600 mt-2 flex items-center gap-1">
                      <Check className="w-4 h-4" /> Copied!
                    </p>
                  )}
                </div>
              </div>

              <div className="flex items-start gap-3 mb-6 p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
                <Checkbox
                  id="saved"
                  checked={savedConfirmation}
                  onCheckedChange={(checked: boolean) => setSavedConfirmation(checked)}
                />
                <Label htmlFor="saved" className="cursor-pointer text-sm">
                  I&apos;ve saved my secret key securely
                </Label>
              </div>

              <Button
                className="w-full bg-blue-600 hover:bg-blue-700"
                size="lg"
                onClick={handleContinue}
                disabled={!savedConfirmation}
              >
                Continue to Dashboard
              </Button>
            </Card>
          </motion.div>
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout>
      <div className="max-w-2xl mx-auto">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
        >
          <Card className="p-8">
            <h1 className="text-2xl font-bold mb-2 text-gray-900 dark:text-white">
              Create New Application
            </h1>
            <p className="text-gray-600 dark:text-gray-400 mb-6">
              Set up a new application to start sending secure messages
            </p>

            <form onSubmit={(e) => {
              e.preventDefault();
              if (!formData.name) return;
              createAppMutation.mutate({
                name: formData.name,
                description: formData.description || undefined,
              });
            }} className="space-y-6">
              <div>
                <Label htmlFor="name">Application Name *</Label>
                <Input
                  id="name"
                  value={formData.name}
                  onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                  placeholder="e.g., Production API"
                  required
                  maxLength={50}
                />
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                  {formData.name.length}/50 characters
                </p>
              </div>

              <div>
                <Label htmlFor="description">Description (optional)</Label>
                <Textarea
                  id="description"
                  value={formData.description}
                  onChange={(e) => setFormData({ ...formData, description: e.target.value })}
                  placeholder="Describe what this application is for..."
                  rows={3}
                />
              </div>

              <div className="flex gap-4">
                <Button
                  type="button"
                  variant="outline"
                  className="flex-1"
                  onClick={() => router.push('/applications')}
                  disabled={createAppMutation.isPending}
                >
                  Cancel
                </Button>
                <Button
                  type="submit"
                  className="flex-1 bg-blue-600 hover:bg-blue-700"
                  disabled={createAppMutation.isPending}
                >
                  {createAppMutation.isPending ? 'Creating…' : 'Create Application'}
                </Button>
              </div>
            </form>
          </Card>
        </motion.div>
      </div>
    </DashboardLayout>
  );
}
