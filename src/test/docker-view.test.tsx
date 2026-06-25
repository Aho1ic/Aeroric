import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import {
  DockerServiceView,
  isIgnorableDockerRefreshError,
} from "../components/docker/DockerServiceView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
}));

describe("DockerServiceView image deletion", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(confirm).mockReset();
  });

  it("keeps the image table visible when post-delete refresh fails", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    const resources = {
      containers: [],
      images: [
        {
          id: "sha256:123",
          repository: "repo/app",
          tag: "latest",
          digest: "sha256:abc",
          createdSince: "2 weeks ago",
          size: "8.2GB",
        },
      ],
    };

    vi.mocked(invoke)
      .mockResolvedValueOnce(resources)
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(
        "Authorized users only. All activities may be monitored and reported. Connection to 192.168.10.100 closed.",
      );

    render(React.createElement(I18nProvider, null, React.createElement(DockerServiceView)));

    await user.click(screen.getByRole("button", { name: /Images/i }));
    await screen.findByText("repo/app");
    await user.click(screen.getByTitle("Delete image"));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("docker_delete_image", {
        remote: null,
        image: "repo/app:latest",
      });
    });

    expect(confirm).toHaveBeenCalledWith(
      "Delete image repo/app:latest?",
      expect.objectContaining({
        title: "Delete image",
      }),
    );
    expect(screen.queryByText("Failed to load Docker resources")).not.toBeInTheDocument();
    expect(screen.getByText("No Docker images")).toBeInTheDocument();
  });

  it("deletes dangling images by id instead of an invalid <none>:<none> reference", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    const resources = {
      containers: [],
      images: [
        {
          id: "5f91320248dc",
          repository: "<none>",
          tag: "<none>",
          digest: "<none>",
          createdSince: "2 days ago",
          size: "1.2GB",
        },
      ],
    };

    vi.mocked(invoke)
      .mockResolvedValueOnce(resources)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({ containers: [], images: [] });

    render(React.createElement(I18nProvider, null, React.createElement(DockerServiceView)));

    await user.click(screen.getByRole("button", { name: /Images/i }));
    await screen.findByText("5f91320248dc");
    await user.click(screen.getByTitle("Delete image"));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("docker_delete_image", {
        remote: null,
        image: "5f91320248dc",
      });
    });
    expect(confirm).toHaveBeenCalledWith("Delete image 5f91320248dc?", expect.any(Object));
  });

  it("waits for image delete confirmation and does not delete when cancelled", async () => {
    const user = userEvent.setup();
    const resources = {
      containers: [],
      images: [
        {
          id: "sha256:123",
          repository: "repo/app",
          tag: "latest",
          digest: "sha256:abc",
          createdSince: "2 weeks ago",
          size: "8.2GB",
        },
      ],
    };
    vi.mocked(confirm).mockResolvedValue(false);
    vi.mocked(invoke).mockResolvedValueOnce(resources);

    render(React.createElement(I18nProvider, null, React.createElement(DockerServiceView)));

    await user.click(screen.getByRole("button", { name: /Images/i }));
    await screen.findByText("repo/app");
    await user.click(screen.getByTitle("Delete image"));

    await waitFor(() => expect(confirm).toHaveBeenCalled());
    expect(invoke).not.toHaveBeenCalledWith("docker_delete_image", expect.anything());
  });

  it("opens a tag dialog and submits a new image tag", async () => {
    const user = userEvent.setup();
    const resources = {
      containers: [],
      images: [
        {
          id: "sha256:123",
          repository: "repo/app",
          tag: "latest",
          digest: "sha256:abc",
          createdSince: "2 weeks ago",
          size: "8.2GB",
        },
      ],
    };
    vi.mocked(invoke)
      .mockResolvedValueOnce(resources)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce(resources);

    render(React.createElement(I18nProvider, null, React.createElement(DockerServiceView)));

    await user.click(screen.getByRole("button", { name: /Images/i }));
    await screen.findByText("repo/app");
    await user.click(screen.getByTitle("Tag image"));
    const input = screen.getByRole("textbox");
    await user.clear(input);
    await user.type(input, "repo/app:stable");
    await user.click(screen.getByRole("button", { name: "Tag" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("docker_tag_image", {
        remote: null,
        source: "repo/app:latest",
        target: "repo/app:stable",
      });
    });
  });

  it("does not reload remote resources when the same SSH connection is recreated by a parent render", async () => {
    const resources = {
      containers: [],
      images: [],
    };
    const remote = {
      id: "ssh-1",
      name: "Prod SSH",
      host: "192.168.10.100",
      port: 22,
      username: "root",
      remotePath: "/srv/app",
      createdAt: 1,
    };

    vi.mocked(invoke).mockResolvedValue(resources);

    const { rerender } = render(
      React.createElement(I18nProvider, null, React.createElement(DockerServiceView, { remote })),
    );

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledTimes(1);
    });

    rerender(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DockerServiceView, { remote: { ...remote } }),
      ),
    );

    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("reloads remote resources when SSH credentials change", async () => {
    const resources = {
      containers: [],
      images: [],
    };
    const remote = {
      id: "ssh-1",
      name: "Prod SSH",
      host: "192.168.10.100",
      port: 22,
      username: "root",
      password: "old-secret",
      remotePath: "/srv/app",
      createdAt: 1,
    };

    vi.mocked(invoke).mockResolvedValue(resources);

    const { rerender } = render(
      React.createElement(I18nProvider, null, React.createElement(DockerServiceView, { remote })),
    );

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledTimes(1);
    });

    rerender(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DockerServiceView, { remote: { ...remote, password: "new-secret" } }),
      ),
    );

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledTimes(2);
    });
    expect(invoke).toHaveBeenLastCalledWith("list_docker_resources", {
      remote: { ...remote, password: "new-secret" },
    });
  });

  it("keeps the original remote target while confirming remote image deletion", async () => {
    const user = userEvent.setup();
    const resources = {
      containers: [],
      images: [
        {
          id: "sha256:123",
          repository: "repo/app",
          tag: "latest",
          digest: "sha256:abc",
          createdSince: "2 weeks ago",
          size: "8.2GB",
        },
      ],
    };
    const remote = {
      id: "ssh-1",
      name: "Prod SSH",
      host: "192.168.10.100",
      port: 22,
      username: "root",
      password: "secret",
      remotePath: "/srv/app",
      createdAt: 1,
    };

    vi.mocked(invoke)
      .mockResolvedValueOnce(resources)
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(
        "Authorized users only. All activities may be monitored and reported. Connection to 192.168.10.100 closed.",
      );

    const { rerender } = render(
      React.createElement(I18nProvider, null, React.createElement(DockerServiceView, { remote })),
    );
    vi.mocked(confirm).mockImplementation(async () => {
      rerender(
        React.createElement(
          I18nProvider,
          null,
          React.createElement(DockerServiceView, { remote: { ...remote, password: undefined } }),
        ),
      );
      return true;
    });

    await user.click(screen.getByRole("button", { name: /Images/i }));
    await screen.findByText("repo/app");
    await user.click(screen.getByTitle("Delete image"));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("docker_delete_image", {
        remote,
        image: "repo/app:latest",
      });
    });
    expect(screen.queryByText("Failed to load Docker resources")).not.toBeInTheDocument();
    expect(screen.getByText("No Docker images")).toBeInTheDocument();
  });

  it("defers parent-driven remote changes until after a delete action settles", async () => {
    const user = userEvent.setup();
    const resources = {
      containers: [],
      images: [
        {
          id: "sha256:123",
          repository: "repo/app",
          tag: "latest",
          digest: "sha256:abc",
          createdSince: "2 weeks ago",
          size: "8.2GB",
        },
      ],
    };
    const remote = {
      id: "ssh-1",
      name: "Prod SSH",
      host: "192.168.10.100",
      port: 22,
      username: "root",
      password: "secret",
      remotePath: "/srv/app",
      createdAt: 1,
    };

    vi.mocked(invoke)
      .mockResolvedValueOnce(resources)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce(resources)
      .mockResolvedValueOnce(resources);

    const { rerender } = render(
      React.createElement(I18nProvider, null, React.createElement(DockerServiceView, { remote })),
    );
    vi.mocked(confirm).mockImplementation(async () => {
      rerender(
        React.createElement(
          I18nProvider,
          null,
          React.createElement(DockerServiceView, { remote: { ...remote, password: undefined } }),
        ),
      );
      return true;
    });

    await user.click(screen.getByRole("button", { name: /Images/i }));
    await screen.findByText("repo/app");
    await user.click(screen.getByTitle("Delete image"));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("docker_delete_image", {
        remote,
        image: "repo/app:latest",
      });
    });
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("list_docker_resources", {
        remote,
      });
    });
  });

  it("classifies SSH login banner refresh failures as ignorable after Docker actions", () => {
    expect(
      isIgnorableDockerRefreshError(
        "Authorized users only. All activities may be monitored and reported. Connection to 192.168.10.100 closed.",
      ),
    ).toBe(true);
    expect(isIgnorableDockerRefreshError("Cannot connect to the Docker daemon")).toBe(false);
  });
});
