import DashboardLayout from '../components/layout/DashboardLayout';
import { Card } from '../components/ui/card';

export default function DocsPage() {
  return (
    <DashboardLayout>
      <h1 className="text-3xl font-bold mb-6 text-gray-900 dark:text-white">Documentation</h1>
      <Card className="p-8 text-center">
        <p className="text-gray-600 dark:text-gray-400">Documentation hub coming soon...</p>
      </Card>
    </DashboardLayout>
  );
}
