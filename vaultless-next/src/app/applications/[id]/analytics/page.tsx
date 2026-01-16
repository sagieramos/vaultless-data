import AnalyticsPage from '../../../../pages/AnalyticsPage';

export default function Page({ params }: { params: { id: string } }) {
  return <AnalyticsPage id={params.id} />;
}
