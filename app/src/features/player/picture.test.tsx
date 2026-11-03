import { cleanup, fireEvent, render } from "@testing-library/react"
import { createRef } from "react"
import { afterEach, describe, expect, it } from "vitest"
import type { VideoStream } from "@/bindings"
import { PlayerPicture } from "./Picture"
import { aspectOf, pictureStyle } from "./picture-shape"

afterEach(cleanup)

const stream = (o: Partial<VideoStream>) =>
  ({ width: 1920, height: 1080, sar: { num: 1, den: 1 }, rotation: 0, ...o }) as VideoStream

describe("pictureStyle", () => {
  it("keeps the shape and takes the largest width the stage holds", () => {
    expect(pictureStyle(16 / 9)).toEqual({ aspectRatio: 16 / 9, width: `min(100cqw, calc(100cqh * ${16 / 9}))` })
    expect(pictureStyle(4 / 3)).toEqual({ aspectRatio: 4 / 3, width: `min(100cqw, calc(100cqh * ${4 / 3}))` })
  })
})

describe("aspectOf", () => {
  it("is the width over the height for square pixels", () => {
    expect(aspectOf(stream({}))).toBeCloseTo(16 / 9)
    expect(aspectOf(stream({ width: 1920, height: 804 }))).toBeCloseTo(1920 / 804)
  })

  it("counts the pixels' own shape", () => {
    expect(aspectOf(stream({ width: 720, height: 480, sar: { num: 32, den: 27 } }))).toBeCloseTo(16 / 9)
  })

  it("turns a file stored on its side", () => {
    expect(aspectOf(stream({ rotation: 90 }))).toBeCloseTo(9 / 16)
    expect(aspectOf(stream({ rotation: -90 }))).toBeCloseTo(9 / 16)
    expect(aspectOf(stream({ rotation: 270 }))).toBeCloseTo(9 / 16)
    expect(aspectOf(stream({ rotation: 180 }))).toBeCloseTo(16 / 9)
  })

  it("reads a pixel shape with a zero part as square", () => {
    expect(aspectOf(stream({ sar: { num: 0, den: 0 } }))).toBeCloseTo(16 / 9)
    expect(aspectOf(stream({ width: 640, height: 480, sar: { num: 0, den: 1 } }))).toBeCloseTo(4 / 3)
  })

  it("falls back to 16:9 for a missing, zero or absurd size", () => {
    expect(aspectOf(null)).toBeCloseTo(16 / 9)
    expect(aspectOf(undefined)).toBeCloseTo(16 / 9)
    expect(aspectOf(stream({ width: 0 }))).toBeCloseTo(16 / 9)
    expect(aspectOf(stream({ height: 0 }))).toBeCloseTo(16 / 9)
    expect(aspectOf(stream({ sar: { num: 65535, den: 1 } }))).toBeCloseTo(16 / 9)
    expect(aspectOf(stream({ sar: { num: 1, den: 65535 } }))).toBeCloseTo(16 / 9)
  })
})

/** The shape the element is laid out at (the DOM writes it as "w / h"). */
function shape(v: HTMLVideoElement): number {
  const [w, h = "1"] = v.style.aspectRatio.split("/")
  return Number(w) / Number(h)
}

/** Gives the element the size a loaded video reports. */
function loaded(v: HTMLVideoElement, width: number, height: number) {
  Object.defineProperty(v, "videoWidth", { configurable: true, get: () => width })
  Object.defineProperty(v, "videoHeight", { configurable: true, get: () => height })
}

describe("PlayerPicture", () => {
  it("starts at the file's shape and takes the shape of what plays once it has loaded", () => {
    const ref = createRef<HTMLVideoElement>()
    const dvd = stream({ width: 720, height: 480, sar: { num: 8, den: 9 } })
    const { container } = render(<PlayerPicture videoRef={ref} src="e01.mp4" video={dvd} />)
    const v = container.querySelector("video")!
    expect(ref.current).toBe(v)
    expect(shape(v)).toBeCloseTo(4 / 3)
    // A preview copy of a 4:3 DVD that was cut 16:9.
    loaded(v, 960, 540)
    fireEvent(v, new Event("loadedmetadata"))
    expect(shape(v)).toBeCloseTo(16 / 9)
  })

  it("keeps the file's shape when what plays reports no size", () => {
    const ref = createRef<HTMLVideoElement>()
    const { container } = render(<PlayerPicture videoRef={ref} src="e01.mp4" video={stream({ width: 1920, height: 804 })} />)
    const v = container.querySelector("video")!
    loaded(v, 0, 0)
    fireEvent(v, new Event("loadedmetadata"))
    expect(shape(v)).toBeCloseTo(1920 / 804)
  })

  it("keeps the classes that hold the picture at its own size where container units aren't known", () => {
    const ref = createRef<HTMLVideoElement>()
    const { container } = render(<PlayerPicture videoRef={ref} src="e01.mp4" video={null} />)
    const v = container.querySelector("video")!
    expect(v.classList.contains("max-h-full")).toBe(true)
    expect(v.classList.contains("max-w-full")).toBe(true)
    expect(v.getAttribute("src")).toBe("e01.mp4")
    expect(shape(v)).toBeCloseTo(16 / 9)
  })
})
