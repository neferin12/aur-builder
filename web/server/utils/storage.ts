import { S3Client, PutObjectCommand } from '@aws-sdk/client-s3'
import { getSignedUrl } from '@aws-sdk/s3-request-presigner'
import { required } from './config'
export async function validateUploadGrant(objectKey: string) {
  const endpoint = new URL(process.env.AB_S3_PUBLIC_ENDPOINT ?? required('AB_S3_ENDPOINT'))
  if (!['http:', 'https:'].includes(endpoint.protocol) || endpoint.username || endpoint.password || endpoint.pathname !== '/' || endpoint.search || endpoint.hash) throw new Error('S3 endpoint must be an HTTP origin')
  const ttl = Number(process.env.AB_UPLOAD_TTL ?? 900)
  if (!Number.isInteger(ttl) || ttl < 60 || ttl > 900) throw new Error('Invalid AB_UPLOAD_TTL')
  const client = new S3Client({ endpoint: endpoint.href, region: process.env.AB_S3_REGION ?? 'garage', forcePathStyle: true,
    credentials: { accessKeyId: required('AB_S3_ACCESS_KEY'), secretAccessKey: required('AB_S3_SECRET_KEY') } })
  // Validate signing configuration before committing. Persist no capability URL;
  // the scheduler renews the grant when dispatching, exactly as for Rust web requests.
  await getSignedUrl(client, new PutObjectCommand({ Bucket: required('AB_S3_BUCKET'), Key: objectKey }), { expiresIn: ttl })
  client.destroy()
}
