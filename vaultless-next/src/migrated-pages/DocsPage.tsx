"use client";
import { useState } from 'react';
import { motion } from 'motion/react';
import {
  Book, Code, Key, Shield, MessageSquare, Smartphone, Globe,
  Server, Cpu, Wifi, Lock, Zap, Users, Mail, ExternalLink,
  Copy, Check, ChevronRight, Search, Menu, X, Handshake
} from 'lucide-react';
import DashboardLayout from '../components/layout/DashboardLayout';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Input } from '../components/ui/input';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';

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
    id: 'clients',
    title: 'Client Management',
    icon: Users,
    description: 'Client registration, authentication & sessions',
    articles: [
      { title: 'Client Authentication', slug: 'client-auth' },
      { title: 'Signup API', slug: 'client-signup' },
      { title: 'Login API', slug: 'client-login' },
      { title: 'Client Sessions', slug: 'client-sessions' },
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
    id: 'websocket',
    title: 'Real-time WebSocket',
    icon: Wifi,
    description: 'WebSocket for real-time messaging',
    articles: [
      { title: 'WebSocket Connection', slug: 'ws-connection' },
      { title: 'Inbound Messages', slug: 'ws-inbound' },
      { title: 'Outbound Notifications', slug: 'ws-outbound' },
      { title: 'Typing Indicators', slug: 'ws-typing' },
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
  curl: `curl -X POST https://api.vaultless.io/api/messages/send \\
  -H "Authorization: Bearer YOUR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "recipient_identifier": "+1234567890",
    "ciphertext": "ENCRYPTED_MESSAGE_BASE64",
    "nonce": "UUID",
    "signature": "SIGNATURE_BASE64",
    "session_id": "session_uuid"
  }'`,
  dart: `// Using vaultless-dart package
import 'package:vaultless/vaultless.dart';

void main() async {
  final client = VaultlessClient(
    apiKey: 'YOUR_API_KEY',
    baseUrl: 'https://api.vaultless.io',
  );

  // Initialize client (generates keys)
  await client.initialize();

  // Send message (encryption and signing happen automatically)
  final message = await client.sendMessage(
    recipientIdentifier: '+1234567890',
    content: 'Hello from Vaultless!',
  );

  print('Message ID: \${message.id}');
}`,
  javascript: `// Using @vaultless/sdk package
import { VaultlessClient } from '@vaultless/sdk';

const client = new VaultlessClient({
  apiKey: 'YOUR_API_KEY'
});

// Initialize client (generates keys)
await client.initialize();

// Send message (encryption and signing happen automatically)
const message = await client.sendMessage({
  recipientIdentifier: '+1234567890',
  content: 'Hello from Vaultless!'
});

console.log(message.message_id);`,
  swift: `// Using vaultless-swift SDK
import VaultlessSDK

let client = VaultlessClient(apiKey: "YOUR_API_KEY")

// Initialize client (generates keys)
try client.initialize()

// Send message (encryption and signing happen automatically)
let message = try await client.messages.send(
    to: "+1234567890",
    content: "Hello from Vaultless!",
    encrypt: true
)

print("Message ID: \\(message.messageId)")`,
  python: `# Using vaultless-python package

from vaultless import VaultlessClient

client = VaultlessClient(api_key="YOUR_API_KEY")

# Initialize client (generates keys)
client.initialize()

# Send message (encryption and signing happen automatically)
message = client.send_message(
    to="+1234567890",
    content="Hello from Vaultless!"
)

print(f"Message ID: {message.message_id}")`,
  go: `// Using vaultless-go SDK

package main

import (
    "github.com/vaultless/sdk-go"
    vaultless "github.com/vaultless/vaultless"
)

func main() {
    client := vaultless.NewClient("YOUR_API_KEY")

    // Initialize client (generates keys)
    client.Initialize()

    // Send message (encryption and signing happen automatically)
    message, err := client.SendMessage(vaultless.SendMessageRequest{
        RecipientIdentifier: "+1234567890",
        Content: "Hello from Vaultless!",
    })

    if err != nil {
        panic(err)
    }

    fmt.Printf("Message ID: %s\\n", message.MessageId)
}`,
  c: `// Using vaultless-c library
#include <vaultless/client.h>
#include <stdio.h>

int main() {
    vaultless_client_t* client = vaultless_client_new(
        "YOUR_API_KEY",
        "https://api.vaultless.io"
    );

    // Initialize client (generates keys)
    vaultless_client_initialize(client);

    // Send message (encryption and signing happen automatically)
    vaultless_message_t* message = vaultless_send_message(client,
        "+1234567890",
        "Hello from Vaultless!"
    );

    printf("Message ID: %s\\n", vaultless_message_id(message));

    vaultless_message_free(message);
    vaultless_client_free(client);

    return 0;
}`,
  rust: `// Using vaultless-rs crate
use vaultless_client::{VaultlessClient, SendMessageRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = VaultlessClient::new(
        "YOUR_API_KEY".to_string(),
        "https://api.vaultless.io".to_string()
    )?;

    let message = client.send_message(SendMessageRequest {
        recipient_identifier: Some("+1234567890".to_string()),
        ciphertext: "ENCRYPTED_MESSAGE_BASE64".to_string(),
        nonce: uuid::Uuid::new_v4(),
        signature: Some("SIGNATURE_BASE64".to_string()),
        session_id: Some("session_uuid".to_string()),
    }).await?;

    println!("Message ID: {}", message.id);
    Ok(())
}`,
  kotlin: `// Using vaultless-kotlin SDK
import io.vaultless.client.VaultlessClient

fun main() {
    val client = VaultlessClient(
        apiKey = "YOUR_API_KEY",
        baseUrl = "https://api.vaultless.io"
    )

    val message = client.sendMessage(
        recipientIdentifier = "+1234567890",
        ciphertext = "ENCRYPTED_MESSAGE_BASE64",
        nonce = UUID.randomUUID(),
        signature = "SIGNATURE_BASE64",
        sessionId = "session_uuid"
    )

    println("Message ID: \${message.id}")
}`,
  java: `// Using vaultless-java SDK
import io.vaultless.client.VaultlessClient;
import java.util.UUID;

public class Example {
    public static void main(String[] args) {
        VaultlessClient client = new VaultlessClient(
            "YOUR_API_KEY",
            "https://api.vaultless.io"
        );

        SendMessageRequest request = new SendMessageRequest();
        request.setRecipientIdentifier("+1234567890");
        request.setCiphertext("ENCRYPTED_MESSAGE_BASE64");
        request.setNonce(UUID.randomUUID());
        request.setSignature("SIGNATURE_BASE64");
        request.setSessionId("session_uuid");

        MessageResponse message = client.sendMessage(request);
        System.out.println("Message ID: " + message.getId());
    }
}`,
};

export default function DocsPage() {
  const [selectedCategory, setSelectedCategory] = useState('getting-started');
  const [selectedArticle, setSelectedArticle] = useState('intro');
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const [copiedCode, setCopiedCode] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState('javascript');

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

        <motion.aside
          initial={{ opacity: 0, x: -20 }}
          animate={{ opacity: 1, x: 0 }}
          className={`lg:w-72 flex-shrink-0 ${mobileMenuOpen ? 'block' : 'hidden lg:block'}`}
        >
          <Card className="p-4 lg:sticky lg:top-24">
            <div className="relative mb-4">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
              <Input placeholder="Search docs..." className="pl-9" />
            </div>

            <nav className="space-y-1">
              {docCategories.map((category) => (
                <button
                  key={category.id}
                  onClick={() => {
                    setSelectedCategory(category.id);
                    setSelectedArticle(category.articles[0].slug);
                  }}
                  className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg text-left transition-colors ${selectedCategory === category.id
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

        <motion.main
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          className="flex-1 lg:ml-6"
        >
          <div className="flex items-center gap-2 text-sm text-gray-500 mb-4">
            <span>Docs</span>
            <ChevronRight className="w-4 h-4" />
            <span className="text-gray-900 dark:text-white">{currentCategory?.title}</span>
            <ChevronRight className="w-4 h-4" />
            <span className="text-blue-600">{currentArticle?.title}</span>
          </div>

          <div className="lg:hidden mb-4">
            <Card className="p-4">
              <h3 className="font-medium mb-3 text-gray-900 dark:text-white">Articles</h3>
              <div className="space-y-1">
                {currentCategory?.articles.map((article) => (
                  <button
                    key={article.slug}
                    onClick={() => setSelectedArticle(article.slug)}
                    className={`w-full flex items-center justify-between px-3 py-2 rounded-lg text-left transition-colors ${selectedArticle === article.slug
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
                    <TabsTrigger value="javascript">JavaScript</TabsTrigger>
                    <TabsTrigger value="swift">Swift</TabsTrigger>
                    <TabsTrigger value="python">Python</TabsTrigger>
                    <TabsTrigger value="go">Go</TabsTrigger>
                    <TabsTrigger value="rust">Rust</TabsTrigger>
                    <TabsTrigger value="kotlin">Kotlin</TabsTrigger>
                    <TabsTrigger value="java">Java</TabsTrigger>
                    <TabsTrigger value="c">C/C++</TabsTrigger>
                    <TabsTrigger value="dart">Dart</TabsTrigger>
                    <TabsTrigger value="curl">cURL</TabsTrigger>
                  </TabsList>
                  {Object.entries(codeExamples).map(([lang, code]) => (
                    <TabsContent key={lang} value={lang}>
                      <div className="relative rounded-lg overflow-hidden bg-[#1a1a2e] p-4">
                        <SyntaxHighlighter
                          language={lang}
                          style={oneDark}
                          customStyle={{ fontSize: '0.875rem' }}
                        >
                          {code}
                        </SyntaxHighlighter>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="absolute top-2 right-2 text-gray-400 hover:text-white bg-gray-800/50 backdrop-blur-sm"
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
                    <li>Get your API key from dashboard</li>
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
          </Card>
        </motion.main>
      </div>
    </DashboardLayout>
  );
}
