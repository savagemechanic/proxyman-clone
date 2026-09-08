import Foundation

public struct HARDocument: Codable, Equatable, Sendable {
    public let log: HARLog
}

public struct HARLog: Codable, Equatable, Sendable {
    public let version: String
    public let creator: HARCreator
    public let entries: [HAREntry]
}

public struct HARCreator: Codable, Equatable, Sendable {
    public let name: String
    public let version: String
}

public struct HAREntry: Codable, Equatable, Sendable {
    public let startedDateTime: String
    public let time: Double
    public let request: HARRequest
    public let response: HARResponse
    public let cache: HARCache
    public let timings: HARTimings
    public let comment: String?
}

public struct HARRequest: Codable, Equatable, Sendable {
    public let method: String
    public let url: String
    public let httpVersion: String
    public let headers: [HARNameValue]
    public let queryString: [HARNameValue]
    public let cookies: [HARNameValue]
    public let headersSize: Int
    public let bodySize: Int64
    public let postData: HARPostData?
}

public struct HARResponse: Codable, Equatable, Sendable {
    public let status: Int
    public let statusText: String
    public let httpVersion: String
    public let headers: [HARNameValue]
    public let cookies: [HARNameValue]
    public let content: HARContent
    public let redirectURL: String
    public let headersSize: Int
    public let bodySize: Int64
}

public struct HARPostData: Codable, Equatable, Sendable {
    public let mimeType: String
    public let text: String
    public let comment: String?
}

public struct HARContent: Codable, Equatable, Sendable {
    public let size: Int64
    public let mimeType: String
    public let text: String?
    public let comment: String?
}

public struct HARNameValue: Codable, Equatable, Sendable {
    public let name: String
    public let value: String
}

public struct HARCache: Codable, Equatable, Sendable {}

public struct HARTimings: Codable, Equatable, Sendable {
    public let send: Double
    public let wait: Double
    public let receive: Double
}

public enum HARExporter {
    public static func document(
        from transactions: [CapturedTransaction],
        creatorVersion: String = "0.1.0"
    ) -> HARDocument {
        HARDocument(
            log: HARLog(
                version: "1.2",
                creator: HARCreator(name: "Proxyman Clone", version: creatorVersion),
                entries: transactions.map(entry(from:))
            )
        )
    }

    public static func data(
        from transactions: [CapturedTransaction],
        creatorVersion: String = "0.1.0"
    ) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        return try encoder.encode(document(from: transactions, creatorVersion: creatorVersion))
    }

    private static func entry(from transaction: CapturedTransaction) -> HAREntry {
        let url = absoluteURL(for: transaction)
        let requestHeaders = transaction.requestHeaders.map { HARNameValue(name: $0.name, value: $0.value) }
        let responseHeaders = transaction.responseHeaders.map { HARNameValue(name: $0.name, value: $0.value) }
        let requestCookies = cookiePairs(from: transaction.requestHeaders, response: false)
        let responseCookies = cookiePairs(from: transaction.responseHeaders, response: true)
        let query = queryPairs(from: url)

        let requestPreview = transaction.requestBodyPreview
        let responsePreview = transaction.responseBodyPreview

        let postData: HARPostData?
        if let text = requestPreview?.text {
            postData = HARPostData(
                mimeType: requestPreview?.contentType ?? "text/plain",
                text: text,
                comment: previewComment(requestPreview)
            )
        } else {
            postData = nil
        }

        let content = HARContent(
            size: saturatedInt64(transaction.responseBodyBytes),
            mimeType: responsePreview?.contentType ?? headerValue("content-type", in: transaction.responseHeaders) ?? "application/octet-stream",
            text: responsePreview?.text,
            comment: previewComment(responsePreview)
        )

        let stateComment = transaction.state == "complete"
            ? nil
            : "Captured transaction state: \(transaction.state). Timing information is not yet available."

        return HAREntry(
            startedDateTime: iso8601(unixMilliseconds: transaction.startedAtUnixMs),
            time: 0,
            request: HARRequest(
                method: transaction.method,
                url: url,
                httpVersion: "HTTP/1.1",
                headers: requestHeaders,
                queryString: query,
                cookies: requestCookies,
                headersSize: -1,
                bodySize: saturatedInt64(transaction.requestBodyBytes),
                postData: postData
            ),
            response: HARResponse(
                status: Int(transaction.statusCode ?? 0),
                statusText: transaction.statusCode == nil ? transaction.state : "",
                httpVersion: "HTTP/1.1",
                headers: responseHeaders,
                cookies: responseCookies,
                content: content,
                redirectURL: headerValue("location", in: transaction.responseHeaders) ?? "",
                headersSize: -1,
                bodySize: saturatedInt64(transaction.responseBodyBytes)
            ),
            cache: HARCache(),
            timings: HARTimings(send: 0, wait: 0, receive: 0),
            comment: stateComment
        )
    }

    private static func absoluteURL(for transaction: CapturedTransaction) -> String {
        let scheme = transaction.scheme == "https" ? "https" : "http"
        let path = transaction.target.hasPrefix("/") ? transaction.target : "/\(transaction.target)"
        return "\(scheme)://\(transaction.host)\(path)"
    }

    private static func queryPairs(from url: String) -> [HARNameValue] {
        guard let components = URLComponents(string: url) else { return [] }
        return (components.queryItems ?? []).map {
            HARNameValue(name: $0.name, value: $0.value ?? "")
        }
    }

    private static func cookiePairs(from headers: [HeaderField], response: Bool) -> [HARNameValue] {
        let target = response ? "set-cookie" : "cookie"
        let values = headers
            .filter { $0.name.caseInsensitiveCompare(target) == .orderedSame }
            .map(\.value)

        if response {
            return values.compactMap { value in
                guard let pairPart = value.split(separator: ";", maxSplits: 1).first else { return nil }
                let pair = String(pairPart).trimmingCharacters(in: .whitespaces)
                guard let equals = pair.firstIndex(of: "=") else { return nil }
                return HARNameValue(
                    name: String(pair[..<equals]).trimmingCharacters(in: .whitespaces),
                    value: String(pair[pair.index(after: equals)...])
                )
            }
        }

        return values.flatMap { value in
            value.split(separator: ";").compactMap { pairPart in
                let pair = String(pairPart).trimmingCharacters(in: .whitespaces)
                guard let equals = pair.firstIndex(of: "=") else { return nil }
                return HARNameValue(
                    name: String(pair[..<equals]),
                    value: String(pair[pair.index(after: equals)...])
                )
            }
        }
    }

    private static func headerValue(_ name: String, in headers: [HeaderField]) -> String? {
        headers.first { $0.name.caseInsensitiveCompare(name) == .orderedSame }?.value
    }

    private static func previewComment(_ preview: BodyPreview?) -> String? {
        guard let preview, preview.truncated else { return nil }
        return "Body preview truncated: \(preview.capturedBytes) of \(preview.totalBytes) bytes captured."
    }

    private static func saturatedInt64(_ value: UInt64) -> Int64 {
        value > UInt64(Int64.max) ? Int64.max : Int64(value)
    }

    private static func iso8601(unixMilliseconds: UInt64) -> String {
        let seconds = Double(unixMilliseconds) / 1_000
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.string(from: Date(timeIntervalSince1970: seconds))
    }
}
