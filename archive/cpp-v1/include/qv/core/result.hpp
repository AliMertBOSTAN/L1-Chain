#pragma once

#include <variant>
#include <string>
#include <optional>
#include <stdexcept>

namespace qv::core {

/// Result<T, E>: Rust-inspired Result type for error handling
/// Represents either a successful value of type T or an error of type E
template <typename T, typename E = std::string>
class Result {
public:
    using value_type = T;
    using error_type = E;

    /// Construct a success result with a value
    explicit Result(const T& value) : data_(value) {}

    /// Construct a success result with a moved value
    explicit Result(T&& value) : data_(std::move(value)) {}

    /// Construct an error result
    explicit Result(const E& error) : data_(error) {}

    /// Construct an error result from a moved error
    explicit Result(E&& error) : data_(std::move(error)) {}

    /// Check if this result is a success
    [[nodiscard]] bool is_ok() const noexcept {
        return std::holds_alternative<T>(data_);
    }

    /// Check if this result is an error
    [[nodiscard]] bool is_err() const noexcept {
        return std::holds_alternative<E>(data_);
    }

    /// Get the value, returns std::optional that is empty on error
    [[nodiscard]] std::optional<T> ok() const noexcept {
        if (is_ok()) {
            return std::get<T>(data_);
        }
        return std::nullopt;
    }

    /// Get the error, returns std::optional that is empty on success
    [[nodiscard]] std::optional<E> err() const noexcept {
        if (is_err()) {
            return std::get<E>(data_);
        }
        return std::nullopt;
    }

    /// Get reference to the value (unchecked)
    /// Undefined behavior if this is an error
    [[nodiscard]] const T& value() const {
        return std::get<T>(data_);
    }

    /// Get mutable reference to the value (unchecked)
    /// Undefined behavior if this is an error
    [[nodiscard]] T& value() {
        return std::get<T>(data_);
    }

    /// Get the error message (unchecked)
    /// Undefined behavior if this is a success
    [[nodiscard]] const E& error() const {
        return std::get<E>(data_);
    }

    /// Get mutable reference to the error (unchecked)
    [[nodiscard]] E& error() {
        return std::get<E>(data_);
    }

    /// Unwrap the value, throws std::runtime_error if this is an error
    [[nodiscard]] T unwrap() const {
        if (is_ok()) {
            return std::get<T>(data_);
        }
        throw std::runtime_error("Called unwrap on an error Result");
    }

    /// Unwrap the value with a custom error message
    [[nodiscard]] T unwrap_or(const T& default_value) const noexcept {
        if (is_ok()) {
            return std::get<T>(data_);
        }
        return default_value;
    }

    /// Map the result value through a function
    template <typename F>
    [[nodiscard]] auto map(F&& fn) const {
        using U = decltype(fn(std::get<T>(data_)));
        if (is_ok()) {
            return Result<U, E>(fn(std::get<T>(data_)));
        }
        return Result<U, E>(std::get<E>(data_));
    }

    /// Map the error through a function
    template <typename F>
    [[nodiscard]] auto map_err(F&& fn) const {
        using E2 = decltype(fn(std::get<E>(data_)));
        if (is_ok()) {
            return Result<T, E2>(std::get<T>(data_));
        }
        return Result<T, E2>(fn(std::get<E>(data_)));
    }

private:
    std::variant<T, E> data_;
};

/// Specialization for Result<void> - success with no value
template <typename E>
class Result<void, E> {
public:
    using value_type = void;
    using error_type = E;

    /// Construct a success result
    static Result<void, E> ok() {
        return Result<void, E>(success_tag{});
    }

    /// Construct an error result
    explicit Result(const E& error) : is_error_(true), error_(error) {}

    /// Construct an error result from a moved error
    explicit Result(E&& error) : is_error_(true), error_(std::move(error)) {}

    /// Check if this result is a success
    [[nodiscard]] bool is_ok() const noexcept {
        return !is_error_;
    }

    /// Check if this result is an error
    [[nodiscard]] bool is_err() const noexcept {
        return is_error_;
    }

    /// Get the error, returns std::optional that is empty on success
    [[nodiscard]] std::optional<E> err() const noexcept {
        if (is_error_) {
            return error_;
        }
        return std::nullopt;
    }

    /// Get the error message (unchecked)
    [[nodiscard]] const E& error() const {
        return error_;
    }

    /// Unwrap, throws if this is an error
    void unwrap() const {
        if (is_error_) {
            throw std::runtime_error("Called unwrap on an error Result");
        }
    }

private:
    struct success_tag {};

    Result(success_tag) : is_error_(false) {}

    bool is_error_ = false;
    E error_{};
};

} // namespace qv::core
