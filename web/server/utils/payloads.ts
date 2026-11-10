import { z } from 'zod'
export const payloadSchema = z.object({
  name: z.string(), version: z.string(), maintainer: z.string(), last_modified: z.number().int(),
  source: z.string().nullable(), subfolder: z.string().nullable(), options: z.string().nullable(),
  environment: z.array(z.object({ name: z.string(), value: z.string() })).nullable(),
})
export const snapshotSchema = z.object({ complete: z.boolean(), files: z.array(z.object({
  path: z.string(), content: z.string().nullable(), hash: z.string(), mode: z.number().int(), size: z.number().int().nonnegative(),
})) })
export const taskSchema = z.object({
  id: z.number().int(), name: z.string(), version: z.string(), candidate_id: z.string().nullable().optional(), revision: z.string().nullable().optional(),
  attempt: z.string(), object_key: z.string(), source: z.string().nullable(), subfolder: z.string().nullable(), options: z.string().nullable(),
  env: payloadSchema.shape.environment,
})
export type Snapshot = z.infer<typeof snapshotSchema>
