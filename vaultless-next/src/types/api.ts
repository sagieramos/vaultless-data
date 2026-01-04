// ============================================================================
// AUTH TYPES
// ============================================================================

export interface UserInfo {
  email: string;
  name?: string;
  emailVerified: boolean;
  isAdmin: boolean;
}

export interface RegisterRequest {
  email: string;
  password: string;
  name?: string;
}

export interface RegisterResponse {
  email: string;
  message: string;
}

export interface LoginRequest {
  email: string;
  password: string;
}

export interface LoginResponse {
  accessToken: string;
  refreshToken: string;
  tokenType: string;
  expiresIn: number;
  user: UserInfo;
}

export interface RefreshTokenRequest {
  refreshToken: string;
}

export interface RefreshTokenResponse {
  accessToken: string;
  refreshToken: string;
  tokenType: string;
  expiresIn: number;
}

export interface VerifyEmailRequest {
  token: string;
}

export interface VerifyEmailResponse {
  message: string;
  email: string;
}

export interface RequestPasswordResetRequest {
  email: string;
}

export interface RequestPasswordResetResponse {
  message: string;
}

export interface ResetPasswordRequest {
  token: string;
  newPassword: string;
}

export interface ResetPasswordResponse {
  message: string;
}

export interface LogoutResponse {
  message: string;
}

export interface UserResponse {
  email: string;
  name?: string;
  avatarUrl?: string;
  emailVerified: boolean;
  isActive: boolean;
  createdAt: string;
  updatedAt: string;
  lastLoginAt?: string;
}

// ============================================================================
// GOOGLE OAUTH TYPES
// ============================================================================

export interface GoogleAuthQuery {
  redirectAfter?: string;
}

export interface GoogleAuthInitResponse {
  authUrl: string;
  state: string;
}

export interface GoogleAuthResponse {
  accessToken: string;
  refreshToken: string;
  tokenType: string;
  expiresIn: number;
  user: UserInfo;
  isNewUser: boolean;
  redirectAfter?: string;
}

// ============================================================================
// APPLICATION TYPES
// ============================================================================

export interface IntegrityConfig {
  proofLevel: string;
  verificationMode: string;
  maxTtlSeconds: number;
  retentionDays: number;
}

export interface ApplicationResponse {
  id: string;
  name: string;
  description?: string;
  isActive: boolean;
  createdAt: string;
  updatedAt: string;
  maxTtlSeconds: number;
  isKeyRotationForced: boolean;
  deletionRequestedAt?: string;
  internalNotes?: string;
  integrityConfig: IntegrityConfig;
}

export interface CreateApplicationRequest {
  name: string;
  description?: string;
}

export interface CreateApplicationResponse {
  application: ApplicationResponse;
  secretKey: string;
  publishableKey: string;
  message: string;
}

export interface PublishableKey {
  id: string;
  applicationId: string;
  keyPrefix: string;
  keyHash: string;
  isActive: boolean;
  createdAt: string;
  lastUsedAt?: string;
  expiresAt?: string;
}

export interface Webhook {
  id: string;
  applicationId: string;
  url: string;
  events: string[];
  secret: string;
  isActive: boolean;
  lastTriggeredAt?: string;
  createdAt: string;
  updatedAt: string;
}

export interface UsageStats {
  msgSent: number;
  msgReceived: number;
  msgProof: number;
  msgStored: number;
  bytesSent: number;
  bytesReceived: number;
  rateHits: number;
  cost: number;
}

export interface LifetimeStats {
  msgSent: number;
  cost: number;
}

export interface ApplicationWithUsage {
  applicationId: string;
  name: string;
  description?: string;
  isActive: boolean;
  createdAt: string;
  updatedAt: string;
  tier?: string;
  monthlyMessageQuota: number;
  rateLimitPerMinute: number;
  messageRetentionSeconds: number;
  currentMonthMessagesSent: number;
  currentMonthMessagesReceived: number;
  currentMonthProofsVerified: number;
  currentMonthBytesStored: number;
  currentMonthBytesSent: number;
  currentMonthBytesReceived: number;
  currentMonthRateLimitHits: number;
  currentMonthCostCents: number;
  lifetimeMessagesSent: number;
  lifetimeCostCents: number;
  quotaUsagePercentage: string;
  publishableKeys: { 0: PublishableKey[] };
  webhooks: { 0: Webhook[] };
}

export interface ApplicationDashboardResponse {
  id: string;
  name: string;
  desc?: string;
  active: boolean;
  created: string;
  updated: string;
  tier?: string;
  monthlyQuota: number;
  rateLimit: number;
  retentionSeconds: number;
  keys: PublishableKey[];
  webhooks: Webhook[];
  quotaUsagePct: number;
  currentMonth: UsageStats;
  lifetime: LifetimeStats;
}

export interface UserUsageSummary {
  totalApplications: number;
  totalMessagesSent: number;
  totalCostCents: number;
  totalQuota: number;
  totalQuotaUsed: number;
}

export interface QuotaWarning {
  applicationId: string;
  applicationName: string;
  currentUsage: number;
  quotaLimit: number;
  usagePercentage: number;
  threshold: number;
  alertLevel: string;
}

export interface PaginationParams {
  page?: number;
  pageSize?: number;
}

export interface PaginatedResponse<T> {
  data: T[];
  totalCount: number;
  page: number;
  pageSize: number;
  totalPages: number;
}

export type PaginatedApplicationsSummary = PaginatedResponse<{
  id: string;
  name: string;
  description?: string;
  isActive: boolean;
  monthlyMessageQuota: number;
  currentMonthMessagesSent: number;
  quotaUsagePercentage: string;
  createdAt: string;
  updatedAt: string;
}>;

export type PaginatedQuotaWarnings = PaginatedResponse<QuotaWarning>;

// ============================================================================
// KEY ROTATION TYPES
// ============================================================================

export interface RotateSecretKeyResponse {
  applicationId: string;
  newSecretKey: string;
  keyPrefix: string;
  createdAt: string;
  oldKeyId: string;
  message: string;
}

export interface RotatePublishableKeyRequest {
  publishableKey?: string;
}

export interface RotatePublishableKeyResponse {
  applicationId: string;
  newPublishableKey: string;
  keyPrefix: string;
  createdAt: string;
  oldKeyId: string;
}

export interface AddPublishableKeyResponse {
  applicationId: string;
  newPublishableKey: string;
  keyPrefix: string;
  createdAt: string;
  totalActivePublishableKeys: number;
}

export interface DeactivatePublishableKeyRequest {
  publishableKey: string;
}

// ============================================================================
// ANALYTICS TYPES
// ============================================================================

export interface QuotaStatusResponse {
  applicationId: string;
  messagesUsed: number;
  messagesLimit: number;
  usagePercentage: number;
  isOverQuota: boolean;
  overageCount: number;
  resetsAt: string;
  alertLevel?: string;
}

export interface CostItem {
  category: string;
  amountCents: number;
  unit: string;
  quantity: number;
}

export interface CostBreakdownResponse {
  totalCostCents: number;
  breakdown: CostItem[];
}

export interface TrendsResponse {
  dailyAverageMessages: number;
  projectedMonthlyCostCents: number;
  quotaTrend: string;
}

export type ExportFormat = 'json' | 'csv';

export interface ExportQuery {
  format: ExportFormat;
}

// ============================================================================
// NOTIFICATION TYPES
// ============================================================================

export interface Notification {
  id: string;
  userId: string;
  title: string;
  message: string;
  notificationType: string;
  severity: string;
  actionUrl?: string;
  metadata?: Record<string, any>;
  isRead: boolean;
  readAt?: string;
  createdAt: string;
  updatedAt: string;
  expiresAt?: string;
}

export interface NotificationQuery {
  isRead?: boolean;
  notificationType?: string;
  severity?: string;
  page?: number;
  pageSize?: number;
}

export interface PaginatedNotifications extends PaginatedResponse<Notification> {
  unreadCount: number;
}

export interface UnreadCountResponse {
  unreadCount: number;
}

export interface NotificationSummary {
  notificationType: string;
  severity: string;
  totalCount: number;
  unreadCount: number;
  latestNotification: string;
}

export interface MarkAllReadResponse {
  success: boolean;
  count: number;
  message: string;
}

export interface DeleteResponse {
  success: boolean;
  message: string;
}

export interface DeleteAllReadResponse {
  success: boolean;
  count: number;
  message: string;
}

// ============================================================================
// COMMON TYPES
// ============================================================================

export interface ApiError {
  code?: string;
  message: string;
  details?: any;
}

export class ApiException extends Error {
  public status?: number;
  public code?: string;

  constructor(message: string, status?: number, code?: string) {
    super(message);
    this.name = 'ApiException';
    this.status = status;
    this.code = code;
  }
}
