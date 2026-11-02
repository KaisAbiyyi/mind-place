export type UiRouteId =
  | "overview" | "conversations" | "curated-memory" | "hive-mind-graph"
  | "files-secure-vault" | "ai-providers-mcp" | "setup-onboarding" | "settings";
export type UiWorkspaceAlias =
  | "conversations" | "library" | "memories" | "documents" | "connections"
  | "projects" | "people" | "topics" | "graph" | "health";
export type UiRouteGroup = "Knowledge OS" | "Authority & Infrastructure" | "System";
export interface UiRouteDefinition {
  readonly id: UiRouteId; readonly label: string; readonly group: UiRouteGroup;
  readonly description: string; readonly moduleId: string; readonly defaultWorkspace: UiWorkspaceAlias;
}
export const UI_ROUTES: readonly UiRouteDefinition[] = [
  { id:"overview",label:"Overview",group:"Knowledge OS",description:"A live summary of the saved archive and recent activity.",moduleId:"overview",defaultWorkspace:"library" },
  { id:"conversations",label:"Conversations",group:"Knowledge OS",description:"Saved conversations, capture history and transcript reader.",moduleId:"conversations",defaultWorkspace:"conversations" },
  { id:"curated-memory",label:"Curated Memory",group:"Knowledge OS",description:"Memories, projects, people and topic investigations.",moduleId:"curated-memory",defaultWorkspace:"memories" },
  { id:"hive-mind-graph",label:"Hive Mind Graph",group:"Knowledge OS",description:"Explore linked memories and their evidence.",moduleId:"hive-mind-graph",defaultWorkspace:"graph" },
  { id:"files-secure-vault",label:"Files & Secure Vault",group:"Authority & Infrastructure",description:"Approved document roots, files and vault access.",moduleId:"files-secure-vault",defaultWorkspace:"documents" },
  { id:"ai-providers-mcp",label:"AI Providers & MCP",group:"Authority & Infrastructure",description:"Provider connections and Model Context Protocol setup.",moduleId:"ai-providers-mcp",defaultWorkspace:"connections" },
  { id:"setup-onboarding",label:"Setup & Onboarding",group:"System",description:"Capture setup and first-run configuration.",moduleId:"setup-onboarding",defaultWorkspace:"connections" },
  { id:"settings",label:"Settings",group:"System",description:"Privacy, security and operational preferences.",moduleId:"settings",defaultWorkspace:"health" },
] as const;
export function getUiRoute(id:UiRouteId):UiRouteDefinition {
  const route=UI_ROUTES.find((item)=>item.id===id); if(!route)throw new Error("MP_UI_UNKNOWN_ROUTE"); return route;
}
export function isUiRouteId(value:string):value is UiRouteId { return UI_ROUTES.some((route)=>route.id===value); }
const WORKSPACE_ROUTE:Readonly<Record<UiWorkspaceAlias,UiRouteId>> = {
  conversations:"conversations",library:"overview",memories:"curated-memory",documents:"files-secure-vault",
  connections:"ai-providers-mcp",projects:"curated-memory",people:"curated-memory",topics:"curated-memory",
  graph:"hive-mind-graph",health:"settings",
};
export function routeForWorkspace(workspace:UiWorkspaceAlias):UiRouteId { return WORKSPACE_ROUTE[workspace]; }
export type UiSurfaceStatus = "idle"|"loading"|"ready"|"empty"|"error"|"permission-required"|"permission-denied"|"locked"|"disconnected"|"revoked";
export type UiPermissionState = "unknown"|"not-required"|"granted"|"required"|"denied";
export type UiVaultState = "not-applicable"|"unlocked"|"locked";
export interface UiModuleError { readonly code:string; readonly message:string; readonly retryable:boolean; }
export interface UiModuleDataState<T> {
  readonly status:UiSurfaceStatus; readonly data:T|null; readonly error:UiModuleError|null;
  readonly permission:UiPermissionState; readonly vault:UiVaultState;
}
export type UiInvoke = <T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
export interface UiCommandRegistration {
  readonly id:string;
  readonly label:string;
  readonly description:string;
  readonly keywords?:readonly string[];
  readonly route?:UiRouteId;
  readonly action?:string;
  readonly workspace?:UiWorkspaceAlias;
}
export interface UiModuleContext {
  readonly document:Document; readonly root:HTMLElement; readonly route:UiRouteId;
  readonly invoke:UiInvoke; readonly signal:AbortSignal; readonly technicalMode:boolean;
  readonly registerCommand:(command:UiCommandRegistration)=>()=>void;
}
export interface UiModuleHandle {
  readonly ready:Promise<void>; state():UiModuleDataState<unknown>; refresh?():Promise<void>; destroy():void;
}
export interface UiModuleRegistration {
  readonly id:string; readonly route:UiRouteId; readonly title:string;
  readonly availability:"available"|"planned"; readonly mount?:(context:UiModuleContext)=>UiModuleHandle;
}
/** Route metadata remains separate from module mounts so feature lanes can integrate independently. */
export const UI_MODULES:readonly UiModuleRegistration[]=UI_ROUTES.map((route)=>({
  id:route.moduleId,route:route.id,title:route.label,availability:"available" as const,
}));
export class UiModuleRegistry {
  private readonly byId=new Map<string,UiModuleRegistration>();
  private readonly byRoute=new Map<UiRouteId,UiModuleRegistration>();
  register(registration:UiModuleRegistration):void {
    const id=registration.id.trim();
    if(!id||id!==registration.id)throw new Error("MP_UI_MODULE_INVALID_ID");
    if(!isUiRouteId(registration.route))throw new Error("MP_UI_MODULE_UNKNOWN_ROUTE");
    if(this.byId.has(id))throw new Error("MP_UI_MODULE_DUPLICATE_ID");
    if(this.byRoute.has(registration.route))throw new Error("MP_UI_MODULE_DUPLICATE_ROUTE");
    const frozen=Object.freeze({...registration}); this.byId.set(id,frozen); this.byRoute.set(registration.route,frozen);
  }
  get(id:string):UiModuleRegistration|undefined{return this.byId.get(id);}
  forRoute(route:UiRouteId):UiModuleRegistration|undefined{return this.byRoute.get(route);}
  list():readonly UiModuleRegistration[]{return [...this.byId.values()];}
}
export function createUiModuleRegistry(registrations:readonly UiModuleRegistration[]=[]):UiModuleRegistry {
  const registry=new UiModuleRegistry(); for(const registration of registrations)registry.register(registration); return registry;
}
