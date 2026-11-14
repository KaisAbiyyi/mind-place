export interface UiRuleViolation {
  code: string;
  file: string;
  line: number;
  detail: string;
}

export interface VerifyUiRulesOptions {
  rootDir?: string;
  files?: Array<{ path: string; content: string }>;
  checkParity?: boolean;
}

export interface VerifyUiRulesResult {
  ok: boolean;
  checkedFiles: number;
  violations: UiRuleViolation[];
}

export function verifyUiRules(options?: VerifyUiRulesOptions): VerifyUiRulesResult;
