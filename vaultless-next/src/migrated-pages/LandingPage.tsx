"use client";
import Link from 'next/link';
import { motion } from 'motion/react';
import { Zap, Lock, DollarSign, BarChart3, Code, Check, ArrowRight, Shield, Gauge, Github, Twitter } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '../components/ui/accordion';

const features = [
  {
    icon: Zap,
    title: '1-Click Integration',
    description: 'Copy-paste code samples and start sending messages in minutes'
  },
  {
    icon: Lock,
    title: 'Zero-Knowledge Security',
    description: 'End-to-end encryption with PASETO tokens and envelope encryption'
  },
  {
    icon: DollarSign,
    title: 'Predictable Pricing',
    description: 'No surprise bills. Clear, transparent pricing you can count on'
  },
  {
    icon: BarChart3,
    title: 'Real-Time Analytics',
    description: 'Monitor usage, track metrics, and optimize performance instantly'
  }
];

const testimonials = [
  {
    name: 'Sarah Chen',
    role: 'CTO at TechFlow',
    content: 'Vaultless cut our integration time from 2 weeks to 30 minutes. The DX is phenomenal.',
    avatar: 'SC'
  },
  {
    name: 'Marcus Johnson',
    role: 'Lead Developer',
    content: 'Finally, a messaging platform that doesn\'t hide costs. Clear pricing and great performance.',
    avatar: 'MJ'
  },
  {
    name: 'Emily Rodriguez',
    role: 'Indie Hacker',
    content: 'As a solo developer, I needed something simple. Vaultless delivered exactly that.',
    avatar: 'ER'
  }
];

const logos = ['TechCorp', 'StartupXYZ', 'DevTools', 'CloudBase', 'DataFlow', 'SecureNet'];

export default function LandingPage() {
  return (
    <div className="min-h-screen bg-white dark:bg-gray-950">
      {/* Header */}
      <header className="border-b border-gray-200 dark:border-gray-800">
        <div className="container mx-auto px-4 py-4 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Shield className="w-8 h-8 text-blue-600" />
            <span className="text-2xl font-bold text-gray-900 dark:text-white">Vaultless</span>
          </div>
          
          <nav className="hidden md:flex items-center gap-6">
            <Link href="/docs" className="text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white">
              Docs
            </Link>
            <a href="#pricing" className="text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white">
              Pricing
            </a>
            <Link href="/login" className="text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white">
              Log in
            </Link>
            <Link href="/register">
              <Button className="bg-blue-600 hover:bg-blue-700">
                Start Building Free
              </Button>
            </Link>
          </nav>
        </div>
      </header>

      {/* Hero Section */}
      <section className="py-20 md:py-32">
        <div className="container mx-auto px-4">
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6 }}
            className="text-center max-w-4xl mx-auto"
          >
            <h1 className="text-5xl md:text-6xl lg:text-7xl font-bold text-gray-900 dark:text-white mb-6">
              Ship Secure Messaging in Minutes, Not Days
            </h1>
            <p className="text-xl md:text-2xl text-gray-600 dark:text-gray-400 mb-8">
              The fastest way to add end-to-end encrypted messaging to your app
            </p>
            
            <div className="flex flex-col sm:flex-row items-center justify-center gap-4 mb-12">
              <Link href="/register">
                <Button size="lg" className="bg-blue-600 hover:bg-blue-700 text-lg px-8">
                  Start Building Free <ArrowRight className="ml-2 w-5 h-5" />
                </Button>
              </Link>
              <Button size="lg" variant="outline" className="text-lg px-8">
                Watch Demo
              </Button>
            </div>

            {/* Trust Badges */}
            <div className="flex flex-wrap items-center justify-center gap-6 text-sm text-gray-600 dark:text-gray-400">
              <Badge variant="outline" className="px-4 py-2">
                <Shield className="w-4 h-4 mr-2" />
                SOC2 Compliant
              </Badge>
              <Badge variant="outline" className="px-4 py-2">
                <Lock className="w-4 h-4 mr-2" />
                256-bit Encryption
              </Badge>
              <Badge variant="outline" className="px-4 py-2">
                <Gauge className="w-4 h-4 mr-2" />
                99.99% Uptime
              </Badge>
            </div>
          </motion.div>
        </div>
      </section>

      {/* Social Proof Strip */}
      <section className="py-12 border-y border-gray-200 dark:border-gray-800 bg-gray-50 dark:bg-gray-900">
        <div className="container mx-auto px-4">
          <p className="text-center text-gray-600 dark:text-gray-400 mb-8">
            Trusted by 10,000+ developers
          </p>
          <div className="flex flex-wrap items-center justify-center gap-12 opacity-50">
            {logos.map((logo) => (
              <span key={logo} className="text-2xl font-bold text-gray-400">{logo}</span>
            ))}
          </div>
        </div>
      </section>

      {/* Features Grid */}
      <section className="py-20">
        <div className="container mx-auto px-4">
          <h2 className="text-4xl font-bold text-center text-gray-900 dark:text-white mb-16">
            Built for Developer Happiness
          </h2>
          
          <div className="grid md:grid-cols-2 lg:grid-cols-4 gap-8">
            {features.map((feature, index) => (
              <motion.div
                key={feature.title}
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5, delay: index * 0.1 }}
                viewport={{ once: true }}
              >
                <Card className="p-6 h-full hover:shadow-lg transition-shadow">
                  <feature.icon className="w-12 h-12 text-blue-600 mb-4" />
                  <h3 className="text-xl font-semibold mb-2 text-gray-900 dark:text-white">
                    {feature.title}
                  </h3>
                  <p className="text-gray-600 dark:text-gray-400">
                    {feature.description}
                  </p>
                </Card>
              </motion.div>
            ))}
          </div>
        </div>
      </section>

      {/* Code Preview */}
      <section className="py-20 bg-gray-950 text-white">
        <div className="container mx-auto px-4">
          <div className="max-w-3xl mx-auto">
            <div className="flex items-center gap-2 mb-8">
              <Code className="w-6 h-6" />
              <h2 className="text-3xl font-bold">Send a message in 3 lines of code</h2>
            </div>
            
            <Card className="bg-gray-900 border-gray-800 p-6 font-mono text-sm">
              <pre className="text-green-400">
                {`// Send a message in 3 lines of code
const vaultless = new Vaultless('pk_live_xxx');
await vaultless.messages.send({
  to: 'user_id',
  ciphertext: '...'
});
console.log('Message sent in 12ms!');`}
              </pre>
            </Card>

            <div className="mt-8 flex items-center gap-4">
              <Check className="w-5 h-5 text-green-500" />
              <span>Zero configuration required</span>
            </div>
            <div className="mt-4 flex items-center gap-4">
              <Check className="w-5 h-5 text-green-500" />
              <span>WebSocket support built-in</span>
            </div>
            <div className="mt-4 flex items-center gap-4">
              <Check className="w-5 h-5 text-green-500" />
              <span>Full TypeScript support</span>
            </div>
          </div>
        </div>
      </section>

      {/* Testimonials */}
      <section className="py-20">
        <div className="container mx-auto px-4">
          <h2 className="text-4xl font-bold text-center text-gray-900 dark:text-white mb-16">
            Loved by Developers
          </h2>
          
          <div className="grid md:grid-cols-3 gap-8">
            {testimonials.map((testimonial, index) => (
              <motion.div
                key={testimonial.name}
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5, delay: index * 0.1 }}
                viewport={{ once: true }}
              >
                <Card className="p-6">
                  <p className="text-gray-600 dark:text-gray-400 mb-4">
                    "{testimonial.content}"
                  </p>
                  <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-full bg-blue-600 flex items-center justify-center text-white font-semibold">
                      {testimonial.avatar}
                    </div>
                    <div>
                      <p className="font-semibold text-gray-900 dark:text-white">{testimonial.name}</p>
                      <p className="text-sm text-gray-600 dark:text-gray-400">{testimonial.role}</p>
                    </div>
                  </div>
                </Card>
              </motion.div>
            ))}
          </div>
        </div>
      </section>

      {/* FAQ */}
      <section className="py-20 bg-gray-50 dark:bg-gray-900">
        <div className="container mx-auto px-4">
          <h2 className="text-4xl font-bold text-center text-gray-900 dark:text-white mb-16">
            Frequently Asked Questions
          </h2>
          
          <div className="max-w-3xl mx-auto">
            <Accordion type="single" collapsible>
              <AccordionItem value="item-1">
                <AccordionTrigger>Is my data really encrypted?</AccordionTrigger>
                <AccordionContent>
                  Yes! Vaultless uses end-to-end encryption with PASETO tokens and envelope encryption. We never have access to your unencrypted messages.
                </AccordionContent>
              </AccordionItem>
              
              <AccordionItem value="item-2">
                <AccordionTrigger>What happens if I exceed my quota?</AccordionTrigger>
                <AccordionContent>
                  We'll notify you when you reach 80% of your quota. You can upgrade your plan at any time, or we'll soft-limit your requests with clear error messages.
                </AccordionContent>
              </AccordionItem>
              
              <AccordionItem value="item-3">
                <AccordionTrigger>Can I migrate from another service?</AccordionTrigger>
                <AccordionContent>
                  Absolutely! We provide migration guides and tools to help you move from other messaging platforms. Our support team is here to help.
                </AccordionContent>
              </AccordionItem>

              <AccordionItem value="item-4">
                <AccordionTrigger>What's included in the free tier?</AccordionTrigger>
                <AccordionContent>
                  The free tier includes 1,000 messages per month, full encryption features, and access to our documentation. Perfect for testing and small projects.
                </AccordionContent>
              </AccordionItem>
            </Accordion>
          </div>
        </div>
      </section>

      {/* CTA Section */}
      <section className="py-20 bg-blue-600 text-white">
        <div className="container mx-auto px-4 text-center">
          <h2 className="text-4xl font-bold mb-6">
            Ready to build something great?
          </h2>
          <p className="text-xl mb-8 opacity-90">
            Join thousands of developers shipping secure messaging today
          </p>
          <Link href="/register">
            <Button size="lg" variant="secondary" className="text-lg px-8">
              Start Building Free <ArrowRight className="ml-2 w-5 h-5" />
            </Button>
          </Link>
        </div>
      </section>

      {/* Footer */}
      <footer className="border-t border-gray-200 dark:border-gray-800 py-12">
        <div className="container mx-auto px-4">
          <div className="grid md:grid-cols-4 gap-8 mb-8">
            <div>
              <div className="flex items-center gap-2 mb-4">
                <Shield className="w-6 h-6 text-blue-600" />
                <span className="text-xl font-bold text-gray-900 dark:text-white">Vaultless</span>
              </div>
              <p className="text-gray-600 dark:text-gray-400 text-sm">
                Secure messaging infrastructure for modern applications
              </p>
            </div>
            
            <div>
              <h4 className="font-semibold mb-4 text-gray-900 dark:text-white">Product</h4>
              <ul className="space-y-2 text-sm text-gray-600 dark:text-gray-400">
                <li><Link href="/docs" className="hover:text-gray-900 dark:hover:text-white">Documentation</Link></li>
                <li><a href="#pricing" className="hover:text-gray-900 dark:hover:text-white">Pricing</a></li>
                <li><a href="#" className="hover:text-gray-900 dark:hover:text-white">Status</a></li>
              </ul>
            </div>
            
            <div>
              <h4 className="font-semibold mb-4 text-gray-900 dark:text-white">Company</h4>
              <ul className="space-y-2 text-sm text-gray-600 dark:text-gray-400">
                <li><a href="#" className="hover:text-gray-900 dark:hover:text-white">About</a></li>
                <li><a href="#" className="hover:text-gray-900 dark:hover:text-white">Blog</a></li>
                <li><a href="#" className="hover:text-gray-900 dark:hover:text-white">Careers</a></li>
              </ul>
            </div>
            
            <div>
              <h4 className="font-semibold mb-4 text-gray-900 dark:text-white">Legal</h4>
              <ul className="space-y-2 text-sm text-gray-600 dark:text-gray-400">
                <li><a href="#" className="hover:text-gray-900 dark:hover:text-white">Privacy</a></li>
                <li><a href="#" className="hover:text-gray-900 dark:hover:text-white">Terms</a></li>
                <li><a href="#" className="hover:text-gray-900 dark:hover:text-white">Security</a></li>
              </ul>
            </div>
          </div>
          
          <div className="border-t border-gray-200 dark:border-gray-800 pt-8 flex flex-col md:flex-row items-center justify-between gap-4">
            <p className="text-sm text-gray-600 dark:text-gray-400">
              © 2025 Vaultless. All rights reserved.
            </p>
            <div className="flex items-center gap-4">
              <a href="#" className="text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white">
                <Github className="w-5 h-5" />
              </a>
              <a href="#" className="text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white">
                <Twitter className="w-5 h-5" />
              </a>
            </div>
          </div>
        </div>
      </footer>
    </div>
  );
}
