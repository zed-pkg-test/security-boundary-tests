// Independently authored TypeSpec authority for the security-boundary canary.
// Generated JSON Schema is comparison evidence only and has no precedence.
import "@typespec/json-schema";
using TypeSpec.JsonSchema;

namespace ZedPkgSecurity;

@id("AuthorityBoundary")
model AuthorityBoundary {
  repository: string;
  enabled: boolean;
  owner?: string;
}
