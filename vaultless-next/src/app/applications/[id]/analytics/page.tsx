import AnalyticsPage from '../../../../pages/AnalyticsPage';

export default async function Page({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return <AnalyticsPage id={id} />;
}
