import Foundation

public enum CurlCommandError: Error, Equatable, Sendable {
    case truncatedRequestBody
    case unavailableRequestBody
}

public enum CurlCommand {
    public static func generate(from transaction: CapturedTransaction) throws -> String {
        let url = absoluteURL(for: transaction)
        var arguments = ["curl", "-X", shellQuote(transaction.method), shellQuote(url)]

        for header in transaction.requestHeaders {
            arguments.append("-H")
            arguments.append(shellQuote("\(header.name): \(header.value)"))
        }

        if transaction.requestBodyBytes > 0 {
            guard let preview = transaction.requestBodyPreview else {
                throw CurlCommandError.unavailableRequestBody
            }
            guard !preview.truncated, preview.totalBytes == transaction.requestBodyBytes else {
                throw CurlCommandError.truncatedRequestBody
            }
            guard let text = preview.text else {
                throw CurlCommandError.unavailableRequestBody
            }
            arguments.append("--data-raw")
            arguments.append(shellQuote(text))
        }

        return arguments.joined(separator: " ")
    }

    public static func shellQuote(_ value: String) -> String {
        if value.isEmpty {
            return "''"
        }
        return "'" + value.replacingOccurrences(of: "'", with: "'\"'\"'") + "'"
    }

    private static func absoluteURL(for transaction: CapturedTransaction) -> String {
        let scheme = transaction.scheme == "https" ? "https" : "http"
        let path = transaction.target.hasPrefix("/") ? transaction.target : "/\(transaction.target)"
        return "\(scheme)://\(transaction.host)\(path)"
    }
}
