#pragma once

#include <algorithm>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>

namespace hog {

// Fila de bytes de produtor único / consumidor único, sem locks. O consumidor é o IOProc,
// que roda em thread de tempo real: ele não pode alocar, travar nem bloquear.
//
// Os índices são monotônicos e só recebem a máscara na hora de indexar. Isso distingue
// "cheio" de "vazio" sem sacrificar um slot: a diferença write - read é o conteúdo real.
class RingBuffer {
public:
    explicit RingBuffer(std::size_t capacityBytes)
        : capacity_(roundUpToPowerOfTwo(capacityBytes)),
          data_(std::make_unique<std::uint8_t[]>(capacity_)) {}

    std::size_t capacity() const { return capacity_; }

    std::size_t availableToRead() const {
        return writeIndex_.load(std::memory_order_acquire) -
               readIndex_.load(std::memory_order_acquire);
    }

    std::size_t availableToWrite() const { return capacity_ - availableToRead(); }

    // Escreve até `bytes`; devolve quanto coube. Só o produtor chama.
    std::size_t write(const void* src, std::size_t bytes) {
        const std::size_t w = writeIndex_.load(std::memory_order_relaxed);
        const std::size_t r = readIndex_.load(std::memory_order_acquire);
        const std::size_t n = std::min(bytes, capacity_ - (w - r));
        if (n == 0) return 0;

        copyWrapped(data_.get(), w & (capacity_ - 1), static_cast<const std::uint8_t*>(src), n,
                    Direction::IntoBuffer);
        writeIndex_.store(w + n, std::memory_order_release);
        return n;
    }

    // Lê até `bytes`; devolve quanto havia. Só o consumidor chama.
    std::size_t read(void* dst, std::size_t bytes) {
        const std::size_t r = readIndex_.load(std::memory_order_relaxed);
        const std::size_t w = writeIndex_.load(std::memory_order_acquire);
        const std::size_t n = std::min(bytes, w - r);
        if (n == 0) return 0;

        copyWrapped(data_.get(), r & (capacity_ - 1), static_cast<std::uint8_t*>(dst), n,
                    Direction::OutOfBuffer);
        readIndex_.store(r + n, std::memory_order_release);
        return n;
    }

private:
    enum class Direction { IntoBuffer, OutOfBuffer };

    // Uma cópia que cruza o fim do buffer vira duas: até a borda, e o resto a partir do início.
    void copyWrapped(std::uint8_t* ring, std::size_t offset, const std::uint8_t* other,
                     std::size_t n, Direction dir) const {
        const std::size_t untilEdge = std::min(n, capacity_ - offset);
        auto* mutableOther = const_cast<std::uint8_t*>(other);
        if (dir == Direction::IntoBuffer) {
            std::memcpy(ring + offset, other, untilEdge);
            std::memcpy(ring, other + untilEdge, n - untilEdge);
        } else {
            std::memcpy(mutableOther, ring + offset, untilEdge);
            std::memcpy(mutableOther + untilEdge, ring, n - untilEdge);
        }
    }

    static std::size_t roundUpToPowerOfTwo(std::size_t n) {
        std::size_t p = 1;
        while (p < n) p <<= 1;
        return p;
    }

    std::size_t capacity_;
    std::unique_ptr<std::uint8_t[]> data_;
    std::atomic<std::size_t> writeIndex_{0};
    std::atomic<std::size_t> readIndex_{0};
};

}  // namespace hog
