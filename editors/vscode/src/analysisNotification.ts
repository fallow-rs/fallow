interface AnalysisNotificationDocument {
  readonly uri: {
    readonly scheme: string;
  };
}

export const shouldAcceptLspAnalysisComplete = (
  documents: ReadonlyArray<AnalysisNotificationDocument>,
): boolean => documents.some((document) => document.uri.scheme === "file");
