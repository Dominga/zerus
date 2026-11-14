/** A team heading is informational, without project or session actions. */
export function AgentTeamHeading(props: { name: string }) {
  return <div class="agent-team-heading" role="heading" aria-level="3">{props.name}</div>;
}
