import { useState } from 'react';
import { motion } from 'motion/react';
import {
  Book, Code, Key, Shield, MessageSquare, Smartphone, Globe,
  Server, Cpu, Wifi, Lock, Zap, Users, Mail, ExternalLink,
  Copy, Check, ChevronRight, Search, Menu, X
} from 'lucide-react';
import DashboardLayout from '../components/layout/DashboardLayout';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Input } from '../components/ui/input';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs';

const docCategories = [
  {
    id: 'getting-started',
    title: 'Getting Started',
    icon: Book,
    description: 'Quickstart guides and installation',
    articles: [
      { title: 'Introduction to Vaultless', slug: 'intro' },
      { title: 'Quickstart Guide', slug: 'quickstart' },
      { title: 'Authentication', slug: 'auth' },
      { title: 'Your First Request', slug: 'first-request' },
    ]
  },
  {
    id: 'messaging',
    title: 'Messaging API',
    icon: MessageSquare,
    description: 'Send and receive secure messages',
    articles: [
      { title: 'Send Messages', slug: 'send-messages' },
      { title: 'Receive Messages', slug: 'receive-messages' },
      { title: 'Message Templates', slug: 'templates' },
      { title: 'Webhooks', slug: 'webhooks' },
      { title: 'Rate Limits', slug: 'rate-limits' },
    ]
  },
  {
    id: 'mobile',
    title: 'Mobile SDKs',
    icon: Smartphone,
    description: 'iOS and Android integration',
    articles: [
      { title: 'iOS SDK Setup', slug: 'ios-setup' },
      { title: 'Android SDK Setup', slug: 'android-setup' },
      { title: 'Push Notifications', slug: 'push-notifications' },
      { title: 'Offline Support', slug: 'offline-support' },
    ]
  },
  {
    id: 'browser',
    title: 'Browser Integration',
    icon: Globe,
    description: 'Web and JavaScript SDK',
    articles: [
      { title: 'NPM Package', slug: 'npm-package' },
      { title: 'CDN Setup', slug: 'cdn-setup' },
      { title: 'React Integration', slug: 'react' },
      { title: 'Vue Integration', slug: 'vue' },
    ]
  },
  {
    id: 'iot',
    title: 'IoT & Devices',
    icon: Cpu,
    description: 'Device attestation and management',
    articles: [
      { title: 'Device Registration', slug: 'device-registration' },
      { title: 'Attestation Protocol', slug: 'attestation' },
      { title: 'Secure Enclave', slug: 'secure-enclave' },
      { title: 'OTA Updates', slug: 'ota-updates' },
    ]
  },
  {
    id: 'security',
    title: 'Security',
    icon: Shield,
    description: 'Encryption and security features',
    articles: [
      { title: 'End-to-End Encryption', slug: 'e2e-encryption' },
      { title: 'Key Management', slug: 'key-management' },
      { title: 'Certificate Rotation', slug: 'cert-rotation' },
      { title: 'Compliance', slug: 'compliance' },
    ]
  },
];

const codeExamples = {
  curl: `curl -X POST https://api.vaultless.io/v1/messages \\
  -H "Authorization: Bearer YOUR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "to": "+1234567890",
    "content": "Hello from Vaultless!",
    "encrypt": true
  }'`,
  node: `import { VaultlessClient } from '@vaultless/sdk';

const client = new VaultlessClient({
  apiKey: process.env.VAULTLESS_API_KEY
});

const message = await client.messages.send({
  to: '+1234567890',
  content: 'Hello from Vaultless!',
  encrypt: true
});

console.log(message.id);`,
  python: `from vaultless import VaultlessClient

client = VaultlessClient(api_key="YOUR_API_KEY")

message = client.messages.send(
    to="+1234567890",
    content="Hello from Vaultless!",
    encrypt=True
)

print(message.id)`,
  swift: `import Vaultless

let client = VaultlessClient(apiKey: "YOUR_API_KEY")

let message = try await client.messages.send(
    to: "+1234567890",
    content: "Hello from Vaultless!",
    encrypt: true
)

print(message.id)`,
};

export default function DocsPage() {
  const [selectedCategory, setSelectedCategory] = useState('getting-started');
  const [selectedArticle, setSelectedArticle] = useState('intro');
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const [copiedCode, setCopiedCode] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState('curl');

  const currentCategory = docCategories.find(c => c.id === selectedCategory);
  const currentArticle = currentCategory?.articles.find(a => a.slug === selectedArticle);

  const copyCode = (code: string, id: string) => {
    navigator.clipboard.writeText(code);
    setCopiedCode(id);
    setTimeout(() => setCopiedCode(null), 2000);
  };

  return (
    <DashboardLayout>
      <div className="flex flex-col lg:flex-row min-h-[calc(100vh-8rem)]">
        {/* Mobile Menu Toggle */}
        <div className="lg:hidden mb-4">
          <Button
            variant="outline"
            onClick={() => setMobileMenuOpen(!mobileMenuOpen)}
            className="w-full"
          >
            {mobileMenuOpen ? <X className="w-4 h-4 mr-2" /> : <Menu className="w-4 h-4 mr-2" />}
            {mobileMenuOpen ? 'Close Menu' : 'Open Menu'}
          </Button>
        </div>

        {/* Sidebar */}
        <motion.aside
          initial={{ opacity: 0, x: -20 }}
          animate={{ opacity: 1, x: 0 }}
          className={`lg:w-72 flex-shrink-0 ${mobileMenuOpen ? 'block' : 'hidden lg:block'}`}
        >
          <Card className="p-4 lg:sticky lg:top-24">
            {/* Search */}
            <div className="relative mb-4">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
              <Input placeholder="Search docs..." className="pl-9" />
            </div>

            {/* Categories */}
            <nav className="space-y-1">
              {docCategories.map((category) => (
                <button
                  key={category.id}
                  onClick={() => {
                    setSelectedCategory(category.id);
                    setSelectedArticle(category.articles[0].slug);
                  }}
                  className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg text-left transition-colors ${
                    selectedCategory === category.id
                      ? 'bg-blue-50 dark:bg-blue-900/20 text-blue-600'
                      : 'text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800'
                  }`}
                >
                  <category.icon className="w-5 h-5" />
                  <div className="flex-1">
                    <div className="font-medium text-sm">{category.title}</div>
                    <div className="text-xs text-gray-500">{category.articles.length} articles</div>
                  </div>
                </button>
              ))}
            </nav>

            {/* Quick Links */}
            <div className="mt-6 pt-6 border-t border-gray-200 dark:border-gray-700">
              <h3 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-3">
                Resources
              </h3>
              <div className="space-y-2">
                <a href="#" className="flex items-center gap-2 text-sm text-gray-600 dark:text-gray-400 hover:text-blue-600">
                  <ExternalLink className="w-4 h-4" />
                  API Reference
                </a>
                <a href="#" className="flex items-center gap-2 text-sm text-gray-600 dark:text-gray-400 hover:text-blue-600">
                  <Code className="w-4 h-4" />
                  GitHub
                </a>
                <a href="#" className="flex items-center gap-2 text-sm text-gray-600 dark:text-gray-400 hover:text-blue-600">
                  <Zap className="w-4 h-4" />
                  Status Page
                </a>
              </div>
            </div>
          </Card>
        </motion.aside>

        {/* Main Content */}
        <motion.main
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          className="flex-1 lg:ml-6"
        >
          {/* Breadcrumb */}
          <div className="flex items-center gap-2 text-sm text-gray-500 mb-4">
            <span>Docs</span>
            <ChevronRight className="w-4 h-4" />
            <span className="text-gray-900 dark:text-white">{currentCategory?.title}</span>
            <ChevronRight className="w-4 h-4" />
            <span className="text-blue-600">{currentArticle?.title}</span>
          </div>

          {/* Article List (Mobile) */}
          <div className="lg:hidden mb-4">
            <Card className="p-4">
              <h3 className="font-medium mb-3 text-gray-900 dark:text-white">Articles</h3>
              <div className="space-y-1">
                {currentCategory?.articles.map((article) => (
                  <button
                    key={article.slug}
                    onClick={() => setSelectedArticle(article.slug)}
                    className={`w-full flex items-center justify-between px-3 py-2 rounded-lg text-left transition-colors ${
                      selectedArticle === article.slug
                        ? 'bg-blue-50 dark:bg-blue-900/20 text-blue-600'
                        : 'text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800'
                    }`}
                  >
                    <span className="text-sm">{article.title}</span>
                    {selectedArticle === article.slug && <Check className="w-4 h-4" />}
                  </button>
                ))}
              </div>
            </Card>
          </div>

          {/* Article Content */}
          <Card className="p-6 lg:p-8">
            {selectedArticle === 'intro' && (
              <div className="prose dark:prose-invert max-w-none">
                <h1 className="text-3xl font-bold text-gray-900 dark:text-white mb-4">
                  Introduction to Vaultless
                </h1>
                <p className="text-lg text-gray-600 dark:text-gray-400 mb-8">
                  Vaultless is a secure messaging platform that provides end-to-end encrypted
                  communication for web, mobile, and IoT applications.
                </p>

                <div className="grid md:grid-cols-2 gap-4 mb-8">
                  <Card className="p-4 bg-blue-50 dark:bg-blue-900/10 border-blue-200">
                    <Zap className="w-8 h-8 text-blue-600 mb-2" />
                    <h3 className="font-semibold text-gray-900 dark:text-white">Fast Integration</h3>
                    <p className="text-sm text-gray-600 dark:text-gray-400">
                      Get started in minutes with our SDKs for all major platforms
                    </p>
                  </Card>
                  <Card className="p-4 bg-green-50 dark:bg-green-900/10 border-green-200">
                    <Lock className="w-8 h-8 text-green-600 mb-2" />
                    <h3 className="font-semibold text-gray-900 dark:text-white">End-to-End Encrypted</h3>
                    <p className="text-sm text-gray-600 dark:text-gray-400">
                      Messages are encrypted before they leave your devices
                    </p>
                  </Card>
                  <Card className="p-4 bg-purple-50 dark:bg-purple-900/10 border-purple-200">
                    <Cpu className="w-8 h-8 text-purple-600 mb-2" />
                    <h3 className="font-semibold text-gray-900 dark:text-white">IoT Ready</h3>
                    <p className="text-sm text-gray-600 dark:text-gray-400">
                      Device attestation and secure communication for IoT
                    </p>
                  </Card>
                  <Card className="p-4 bg-orange-50 dark:bg-orange-900/10 border-orange-200">
                    <Shield className="w-8 h-8 text-orange-600 mb-2" />
                    <h3 className="font-semibold text-gray-900 dark:text-white">Compliance</h3>
                    <p className="text-sm text-gray-600 dark:text-gray-400">
                      SOC2, GDPR, and HIPAA compliant infrastructure
                    </p>
                  </Card>
                </div>

                <h2 className="text-2xl font-semibold text-gray-900 dark:text-white mb-4">
                  Quick Example
                </h2>
                <p className="text-gray-600 dark:text-gray-400 mb-4">
                  Send your first secure message in just a few lines of code:
                </p>

                <Tabs value={activeTab} onValueChange={setActiveTab} className="mb-4">
                  <TabsList>
                    <TabsTrigger value="curl">cURL</TabsTrigger>
                    <TabsTrigger value="node">Node.js</TabsTrigger>
                    <TabsTrigger value="python">Python</TabsTrigger>
                    <TabsTrigger value="swift">Swift</TabsTrigger>
                  </TabsList>
                  {Object.entries(codeExamples).map(([lang, code]) => (
                    <TabsContent key={lang} value={lang}>
                      <div className="relative">
                        <pre className="bg-gray-900 text-gray-100 p-4 rounded-lg overflow-x-auto text-sm">
                          <code>{code}</code>
                        </pre>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="absolute top-2 right-2 text-gray-400 hover:text-white"
                          onClick={() => copyCode(code, lang)}
                        >
                          {copiedCode === lang ? <Check className="w-4 h-4" /> : <Copy className="w-4 h-4" />}
                        </Button>
                      </div>
                    </TabsContent>
                  ))}
                </Tabs>

                <div className="bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-lg p-4 mb-8">
                  <h3 className="font-semibold text-blue-900 dark:text-blue-100 mb-2">
                    Next Steps
                  </h3>
                  <ul className="list-disc list-inside space-y-1 text-blue-800 dark:text-blue-200">
                    <li>Get your API key from the dashboard</li>
                    <li>Install the SDK for your platform</li>
                    <li>Send your first message</li>
                  </ul>
                </div>

                <h2 className="text-2xl font-semibold text-gray-900 dark:text-white mb-4">
                  Supported Platforms
                </h2>
                <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
                  {[
                    { icon: Globe, name: 'Web', color: 'blue' },
                    { icon: Smartphone, name: 'iOS', color: 'gray' },
                    { icon: Smartphone, name: 'Android', color: 'green' },
                    { icon: Cpu, name: 'IoT', color: 'purple' },
                  ].map((platform) => (
                    <div
                      key={platform.name}
                      className="flex flex-col items-center p-4 bg-gray-50 dark:bg-gray-800 rounded-lg"
                    >
                      <platform.icon className={`w-8 h-8 text-${platform.color}-600 mb-2`} />
                      <span className="text-sm font-medium text-gray-900 dark:text-white">{platform.name}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {selectedArticle === 'quickstart' && (
              <div className="prose dark:prose-invert max-w-none">
                <h1 className="text-3xl font-bold text-gray-900 dark:text-white mb-4">
                  Quickstart Guide
                </h1>
                <p className="text-gray-600 dark:text-gray-400 mb-8">
                  Get up and running with Vaultless in under 5 minutes.
                </p>

                <h2 className="text-2xl font-semibold text-gray-900 dark:text-white mb-4">
                  Step 1: Get Your API Key
                </h2>
                <p className="text-gray-600 dark:text-gray-400 mb-4">
                  Create an account at <a href="#" className="text-blue-600 hover:underline">vaultless.io</a> and
                  generate an API key from your dashboard.
                </p>

                <h2 className="text-2xl font-semibold text-gray-900 dark:text-white mb-4">
                  Step 2: Install the SDK
                </h2>
                <div className="bg-gray-900 text-gray-100 p-4 rounded-lg mb-4">
                  <code>npm install @vaultless/sdk</code>
                </div>
                <p className="text-gray-600 dark:text-gray-400 mb-4">
                  Or using Yarn:
                </p>
                <div className="bg-gray-900 text-gray-100 p-4 rounded-lg mb-4">
                  <code>yarn add @vaultless/sdk</code>
                </div>

                <h2 className="text-2xl font-semibold text-gray-900 dark:text-white mb-4">
                  Step 3: Initialize the Client
                </h2>
                <div className="bg-gray-900 text-gray-100 p-4 rounded-lg mb-4">
                  <pre>{`import { VaultlessClient } from '@vaultless/sdk';

const client = new VaultlessClient({
  apiKey: process.env.VAULTLESS_API_KEY
});`}</pre>
                </div>

                <h2 className="text-2xl font-semibold text-gray-900 dark:text-white mb-4">
                  Step 4: Send a Message
                </h2>
                <div className="bg-gray-900 text-gray-100 p-4 rounded-lg mb-4">
                  <pre>{`const message = await client.messages.send({
  to: '+1234567890',
  content: 'Hello, Vaultless!',
  encrypt: true
});

console.log(message.id);`}</pre>
                </div>

                <div className="bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded-lg p-4">
                  <p className="text-green-800 dark:text-green-200">
                    Congratulations! You've sent your first secure message with Vaultless.
                  </p>
                </div>
              </div>
            )}

            {/* Default content for other articles */}
            {selectedArticle !== 'intro' && selectedArticle !== 'quickstart' && (
              <div className="prose dark:prose-invert max-w-none">
                <h1 className="text-3xl font-bold text-gray-900 dark:text-white mb-4">
                  {currentArticle?.title}
                </h1>
                <p className="text-gray-600 dark:text-gray-400 mb-8">
                  This documentation is being written. Check back soon for detailed guides.
                </p>

                <div className="bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800 rounded-lg p-4">
                  <p className="text-yellow-800 dark:text-yellow-200">
                    <strong>Coming Soon:</strong> Detailed documentation for {currentArticle?.title.toLowerCase()}.
                  </p>
                </div>

                <h2 className="text-2xl font-semibold text-gray-900 dark:text-white mt-8 mb-4">
                  While You Wait
                </h2>
                <p className="text-gray-600 dark:text-gray-400 mb-4">
                  Check out these related articles:
                </p>
                <ul className="list-disc list-inside space-y-2 text-gray-600 dark:text-gray-400">
                  {currentCategory?.articles.filter(a => a.slug !== selectedArticle).map((article) => (
                    <li key={article.slug}>
                      <button
                        onClick={() => setSelectedArticle(article.slug)}
                        className="text-blue-600 hover:underline"
                      >
                        {article.title}
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {/* Article Navigation */}
            <div className="mt-12 pt-6 border-t border-gray-200 dark:border-gray-700 flex items-center justify-between">
              {currentCategory?.articles.findIndex(a => a.slug === selectedArticle) > 0 ? (
                <Button
                  variant="outline"
                  onClick={() => {
                    const index = currentCategory!.articles.findIndex(a => a.slug === selectedArticle);
                    setSelectedArticle(currentCategory!.articles[index - 1].slug);
                  }}
                >
                  ← Previous
                </Button>
              ) : (
                <div />
              )}
              {currentCategory?.articles.findIndex(a => a.slug === selectedArticle) <
              currentCategory!.articles.length - 1 ? (
                <Button
                  onClick={() => {
                    const index = currentCategory!.articles.findIndex(a => a.slug === selectedArticle);
                    setSelectedArticle(currentCategory!.articles[index + 1].slug);
                  }}
                >
                  Next →
                </Button>
              ) : (
                <Button variant="outline" onClick={() => {
                  const nextCategoryIndex = docCategories.findIndex(c => c.id === selectedCategory) + 1;
                  if (nextCategoryIndex < docCategories.length) {
                    setSelectedCategory(docCategories[nextCategoryIndex].id);
                    setSelectedArticle(docCategories[nextCategoryIndex].articles[0].slug);
                  }
                }}>
                  Next Section →
                </Button>
              )}
            </div>
          </Card>
        </motion.main>
      </div>
    </DashboardLayout>
  );
}
