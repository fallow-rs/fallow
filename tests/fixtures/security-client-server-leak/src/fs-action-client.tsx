"use client";
import { auditTemplate } from "./fs-action-mod";

export function AuditView() {
  return <pre>{auditTemplate}</pre>;
}
