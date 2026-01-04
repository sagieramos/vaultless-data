import { useState } from 'react';
import { motion } from 'motion/react';
import {
  Copy, Check, Eye, EyeOff, Plus, RotateCcw, Trash2,
  Shield, AlertTriangle, Key, Clock
} from 'lucide-react';
import { Button } from '../components/ui/button';
import { Input } from '../components/ui/input';
import { Label } from '../components/ui/label';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '../components/ui/dialog';
import { toast } from 'sonner';
import DashboardLayout from '../components/layout/DashboardLayout';

const apiKeys = [
  {
    id: '1',
    name: 'Production Key',
    prefix: 'sk_live_',
    createdAt: '2024-11-15',
    lastUsed: '2 minutes ago',
    status: 'active',
    type: 'secret'
  },
  {
    id: '2',
    name: 'Development Key',
    prefix: 'sk_test_',
    createdAt: '2024-12-01',
    lastUsed: '1 hour ago',
    status: 'active',
    type: 'secret'
  },
  {
    id: '3',
    name: 'Mobile App Key',
    prefix: 'pk_live_',
    createdAt: '2024-12-10',
    lastUsed: '5 minutes ago',
    status: 'active',
    type: 'publishable'
  },
  {
    id: '4',
    name: 'Legacy Key',
    prefix: 'sk_live_',
    createdAt: '2024-10-01',
    lastUsed: '30 days ago',
    status: 'inactive',
    type: 'secret'
  }
];

export default function ApiKeysPage() {
  const [copied, setCopied] = useState<Record<string, boolean>>({});
  const [revealed, setRevealed] = useState<Record<string, boolean>>({});
  const [rotateDialogOpen, setRotateDialogOpen] = useState(false);
  const [selectedKey, setSelectedKey] = useState<typeof apiKeys[0] | null>(null);
  const [rotateConfirm, setRotateConfirm] = useState('');

  const handleCopy = (text: string, id: string) => {
    navigator.clipboard.writeText(text);
    setCopied({ ...copied, [id]: true });
    toast.success('Copied to clipboard!');
    setTimeout(() => setCopied({ ...copied, [id]: false }), 2000);
  };

  const handleReveal = (id: string) => {
    setRevealed({ ...revealed, [id]: !revealed[id] });
  };

  const maskKey = (key: string) => {
    if (key.startsWith('sk_')) {
      return `${key.slice(0, 9)}••••••••••••••••••••••••`;
    }
    return `${key.slice(0, 8)}••••••••••••••••••••••••••`;
  };

  const handleRotate = (key: typeof apiKeys[0]) => {
    setSelectedKey(key);
    setRotateDialogOpen(true);
    setRotateConfirm('');
  };

  const confirmRotate = () => {
    if (rotateConfirm === 'ROTATE' && selectedKey) {
      toast.success(`Key "${selectedKey.name}" rotated successfully!`);
      setRotateDialogOpen(false);
      setSelectedKey(null);
      setRotateConfirm('');
    }
  };

  return (
    <DashboardLayout>
      <div className="mb-8 flex flex-col md:flex-row md:items-center md:justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white">API Keys</h1>
          <p className="text-gray-600 dark:text-gray-400 mt-1">
            Manage your API keys for authentication and access
          </p>
        </div>
        <Button className="bg-blue-600 hover:bg-blue-700">
          <Plus className="w-4 h-4 mr-2" />
          Create New Key
        </Button>
      </div>

      {/* Security Notice */}
      <Card className="p-4 mb-6 bg-yellow-50 dark:bg-yellow-900/20 border-yellow-200 dark:border-yellow-800">
        <div className="flex items-start gap-3">
          <AlertTriangle className="w-5 h-5 text-yellow-600 flex-shrink-0 mt-0.5" />
          <div>
            <h3 className="font-semibold text-yellow-900 dark:text-yellow-100 mb-1">
              Security Reminder
            </h3>
            <p className="text-sm text-yellow-800 dark:text-yellow-200">
              Secret keys are only shown once upon creation. Store them securely.
              Never share your secret keys in client-side code or public repositories.
            </p>
          </div>
        </div>
      </Card>

      {/* Keys List */}
      <div className="space-y-4">
        {apiKeys.map((key, index) => (
          <motion.div
            key={key.id}
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: index * 0.1 }}
          >
            <Card className="p-6">
              <div className="flex flex-col lg:flex-row lg:items-center gap-4">
                {/* Key Info */}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-3 mb-2">
                    <div className={`w-10 h-10 rounded-lg flex items-center justify-center ${
                      key.type === 'secret'
                        ? 'bg-red-100 dark:bg-red-900/20'
                        : 'bg-blue-100 dark:bg-blue-900/20'
                    }`}>
                      <Key className={`w-5 h-5 ${
                        key.type === 'secret'
                          ? 'text-red-600'
                          : 'text-blue-600'
                      }`} />
                    </div>
                    <div>
                      <h3 className="font-semibold text-gray-900 dark:text-white">
                        {key.name}
                      </h3>
                      <div className="flex items-center gap-2 mt-1">
                        <Badge variant={key.type === 'secret' ? 'destructive' : 'default'}>
                          {key.type === 'secret' ? 'Secret Key' : 'Publishable Key'}
                        </Badge>
                        <Badge variant={key.status === 'active' ? 'secondary' : 'outline'}>
                          {key.status}
                        </Badge>
                      </div>
                    </div>
                  </div>

                  {/* Key Display */}
                  <div className="mt-4">
                    <Label className="text-xs text-gray-600 dark:text-gray-400 mb-2 block">
                      Key Value
                    </Label>
                    <div className="flex items-center gap-2">
                      <Input
                        readOnly
                        value={revealed[key.id] ? key.prefix + 'abc123xyz789def456ghi012jkl345mno678pqr901stu234vwx567yza890' : maskKey(key.prefix + 'abc123xyz789def456ghi012jkl345mno678pqr901stu234vwx567yza890')}
                        className="font-mono text-sm bg-gray-50 dark:bg-gray-900"
                      />
                      <Button
                        variant="outline"
                        size="icon"
                        onClick={() => handleReveal(key.id)}
                        title={revealed[key.id] ? 'Hide key' : 'Reveal key'}
                      >
                        {revealed[key.id] ? (
                          <EyeOff className="w-4 h-4" />
                        ) : (
                          <Eye className="w-4 h-4" />
                        )}
                      </Button>
                      <Button
                        variant="outline"
                        size="icon"
                        onClick={() => handleCopy(key.prefix + 'abc123xyz789def456ghi012jkl345mno678pqr901stu234vwx567yza890', key.id)}
                        title="Copy to clipboard"
                      >
                        {copied[key.id] ? (
                          <Check className="w-4 h-4 text-green-600" />
                        ) : (
                          <Copy className="w-4 h-4" />
                        )}
                      </Button>
                    </div>
                  </div>

                  {/* Metadata */}
                  <div className="flex flex-wrap items-center gap-4 mt-4 text-sm text-gray-600 dark:text-gray-400">
                    <span className="flex items-center gap-1">
                      <Clock className="w-4 h-4" />
                      Created {key.createdAt}
                    </span>
                    <span>•</span>
                    <span>Last used {key.lastUsed}</span>
                    {key.type === 'secret' && (
                      <>
                        <span>•</span>
                        <span className="text-orange-600 dark:text-orange-400">
                          Server-side only
                        </span>
                      </>
                    )}
                  </div>
                </div>

                {/* Actions */}
                <div className="flex items-center gap-2">
                  {key.type === 'secret' && (
                    <Button
                      variant="outline"
                      onClick={() => handleRotate(key)}
                    >
                      <RotateCcw className="w-4 h-4 mr-2" />
                      Rotate
                    </Button>
                  )}
                  {key.type === 'publishable' && key.status === 'active' && (
                    <Button
                      variant="outline"
                      className="text-red-600 hover:text-red-700 hover:bg-red-50"
                    >
                      <Trash2 className="w-4 h-4 mr-2" />
                      Deactivate
                    </Button>
                  )}
                </div>
              </div>
            </Card>
          </motion.div>
        ))}
      </div>

      {/* Empty State (if no keys) */}
      {apiKeys.length === 0 && (
        <Card className="p-12 text-center">
          <div className="w-20 h-20 bg-gray-100 dark:bg-gray-800 rounded-full flex items-center justify-center mx-auto mb-6">
            <Shield className="w-10 h-10 text-gray-400" />
          </div>
          <h2 className="text-2xl font-bold mb-2 text-gray-900 dark:text-white">
            No API keys yet
          </h2>
          <p className="text-gray-600 dark:text-gray-400 mb-6">
            Create your first API key to start integrating with Vaultless
          </p>
          <Button className="bg-blue-600 hover:bg-blue-700">
            <Plus className="w-5 h-5 mr-2" />
            Create Your First Key
          </Button>
        </Card>
      )}

      {/* Rotate Key Dialog */}
      <Dialog open={rotateDialogOpen} onOpenChange={setRotateDialogOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Rotate API Key</DialogTitle>
            <DialogDescription>
              This will create a new key and deactivate the current one.
              All existing sessions using this key will be invalidated.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-4">
            <div className="p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg">
              <div className="flex items-start gap-3">
                <AlertTriangle className="w-5 h-5 text-red-600 flex-shrink-0 mt-0.5" />
                <div className="text-sm text-red-800 dark:text-red-200">
                  <p className="font-semibold mb-1">Warning: This action cannot be undone</p>
                  <p>Any applications using this key will stop working immediately.</p>
                </div>
              </div>
            </div>

            <div>
              <Label htmlFor="confirm">Type ROTATE to confirm</Label>
              <Input
                id="confirm"
                value={rotateConfirm}
                onChange={(e) => setRotateConfirm(e.target.value)}
                placeholder="Type ROTATE"
                className="font-mono"
              />
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setRotateDialogOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={confirmRotate}
              disabled={rotateConfirm !== 'ROTATE'}
            >
              <RotateCcw className="w-4 h-4 mr-2" />
              Rotate Key
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </DashboardLayout>
  );
}
