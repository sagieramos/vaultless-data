import { Link } from 'react-router-dom';
import { Home, Search } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';

export default function NotFoundPage() {
  return (
    <div className="min-h-screen bg-gray-50 dark:bg-gray-950 flex items-center justify-center p-4">
      <Card className="p-12 text-center max-w-md">
        <div className="w-20 h-20 bg-gray-100 dark:bg-gray-800 rounded-full flex items-center justify-center mx-auto mb-6">
          <Search className="w-10 h-10 text-gray-400" />
        </div>
        
        <h1 className="text-6xl font-bold text-gray-900 dark:text-white mb-2">404</h1>
        <h2 className="text-2xl font-semibold mb-4 text-gray-900 dark:text-white">
          Page Not Found
        </h2>
        <p className="text-gray-600 dark:text-gray-400 mb-8">
          Sorry, we couldn't find the page you're looking for.
        </p>
        
        <Link to="/dashboard">
          <Button className="bg-blue-600 hover:bg-blue-700">
            <Home className="w-4 h-4 mr-2" />
            Go Back Home
          </Button>
        </Link>
      </Card>
    </div>
  );
}
